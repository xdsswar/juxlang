package dev.jux.intellij.templates

import com.intellij.codeInsight.template.TemplateManager
import com.intellij.codeInsight.template.impl.ConstantNode
import com.intellij.codeInsight.template.postfix.templates.PostfixTemplate
import com.intellij.codeInsight.template.postfix.templates.PostfixTemplateProvider
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import dev.jux.intellij.intentions.JuxSwitchCases
import dev.jux.intellij.resolve.JuxTypeEngine
import dev.jux.intellij.resolve.JuxTypeInference

/**
 * Postfix templates: `expr.if`, `expr.var`, `expr.for`, and friends — type the
 * value first, then say what to do with it. The set and the behaviour follow
 * IntelliJ's Java postfix templates:
 *
 *  - statement templates (`.if`, `.for`, `.return`, ...) are offered only
 *    where a statement can start, so `x = y.if` offers nothing;
 *  - templates are filtered by what the expression is: `.if` wants a boolean,
 *    `.for` something iterable, `.null` something that can be null. The shape
 *    is read cheaply ([Shape]); an expression it cannot judge keeps every
 *    template, because hiding a valid one is worse than showing an extra one;
 *  - names a template introduces (`.var`, `.for`, `.fori`) are live template
 *    variables with a suggested value, so Tab walks through them.
 *
 * The plugin's own completion stands down while `juxc-lsp` is serving, but
 * postfix templates are a separate extension point that keeps working either
 * way.
 *
 * The expression is found by scanning backwards from the `.`, tracking bracket
 * depth, so `xs[0].if` and `f(a, b).var` take the whole expression rather than
 * the last word.
 */
class JuxPostfixTemplateProvider : PostfixTemplateProvider {

    override fun getId(): String = "jux.postfix"

    override fun getPresentableName(): String = "Jux"

    override fun getTemplates(): MutableSet<PostfixTemplate> = TEMPLATES.toMutableSet()

    /** `.` starts a postfix key; the key itself ends at any non-word character. */
    override fun isTerminalSymbol(currentChar: Char): Boolean =
        currentChar == '.' || currentChar == '!'

    override fun preExpand(file: PsiFile, editor: Editor) = Unit

    override fun afterExpand(file: PsiFile, editor: Editor) = Unit

    /** No speculative copy needed: expansion is a plain document edit. */
    override fun preCheck(copyFile: PsiFile, realEditor: Editor, currentOffset: Int): PsiFile =
        copyFile

    private companion object {
        /**
         * `.switch` on an enum or a sealed value writes every arm, the way
         * Java's `.switch` fills in an enum's constants: one `case` per
         * constant or subtype, with the caret in the first arm. Anything else
         * keeps the plain `switch (expr) { }`.
         */
        fun switchWithArms(file: PsiFile, start: Int, end: Int): String? {
            val expr = JuxPostfixTemplate.expressionAt(file, start, end) ?: return null
            val labels = JuxSwitchCases.labelsFor(JuxTypeEngine.typeOf(expr))
            if (labels.isEmpty()) return null
            val arms = labels.mapIndexed { i, label ->
                val inside = if (i == 0) "        \$END\$\n" else ""
                "    case ${label.text} -> {\n$inside    }\n"
            }.joinToString("")
            return "switch (\$EXPR\$) {\n$arms}"
        }

        val STMT = JuxPostfixTemplate.Position.STATEMENT
        val EXPR = JuxPostfixTemplate.Position.EXPRESSION

        /**
         * `$EXPR$` is replaced by the expression before the dot and `$END$`
         * marks where the caret lands. Upper-case `$NAME$`-style markers are
         * live template variables.
         *
         * Names carry no leading dot: [PostfixTemplate] builds its `key` as
         * `"." + name`, so writing `".if"` here would register `..if`.
         */
        val TEMPLATES: Set<PostfixTemplate> = setOf(
            // ---- conditions: want a boolean --------------------------------
            JuxPostfixTemplate("jux.if", "if", "if (expr) { }", "if (\$EXPR\$) {\n    \$END\$\n}", STMT) { it.maybeBool },
            JuxPostfixTemplate("jux.else", "else", "if (!expr) { }", "if (!(\$EXPR\$)) {\n    \$END\$\n}", STMT) { it.maybeBool },
            JuxPostfixTemplate("jux.while", "while", "while (expr) { }", "while (\$EXPR\$) {\n    \$END\$\n}", STMT) { it.maybeBool },
            JuxPostfixTemplate("jux.not", "not", "!expr", "!\$EXPR_NOT\$\$END\$", EXPR) { it.maybeBool },
            JuxPostfixTemplate("jux.assert", "assert", "assert(expr);", "assert(\$EXPR\$);\$END\$", STMT) { it.maybeBool },

            // ---- null checks: want something that can be null ---------------
            JuxPostfixTemplate("jux.null", "null", "if (expr == null) { }", "if (\$EXPR\$ == null) {\n    \$END\$\n}", STMT) { it.maybeNull },
            JuxPostfixTemplate("jux.notnull", "notnull", "if (expr != null) { }", "if (\$EXPR\$ != null) {\n    \$END\$\n}", STMT) { it.maybeNull },
            JuxPostfixTemplate("jux.nn", "nn", "if (expr != null) { }", "if (\$EXPR\$ != null) {\n    \$END\$\n}", STMT) { it.maybeNull },

            // ---- loops -----------------------------------------------------
            JuxPostfixTemplate(
                "jux.for", "for", "for (var item : expr) { }",
                "for (var \$ITEM\$ : \$EXPR\$) {\n    \$END\$\n}", STMT,
                listOf(JuxPostfixTemplate.Variable("ITEM") { it.itemName }),
            ) { it.maybeIterable },
            JuxPostfixTemplate(
                "jux.fori", "fori", "for (int i = 0; i < expr; i++) { }",
                "for (int \$I\$ = 0; \$I\$ < \$BOUND\$; \$I\$++) {\n    \$END\$\n}", STMT,
                listOf(JuxPostfixTemplate.Variable("I") { "i" }),
            ) { it.maybeCountable },
            JuxPostfixTemplate(
                "jux.forr", "forr", "for (int i = expr - 1; i >= 0; i--) { }",
                "for (int \$I\$ = \$BOUND\$ - 1; \$I\$ >= 0; \$I\$--) {\n    \$END\$\n}", STMT,
                listOf(JuxPostfixTemplate.Variable("I") { "i" }),
            ) { it.maybeCountable },

            // ---- values into statements --------------------------------------
            JuxPostfixTemplate(
                "jux.var", "var", "var name = expr;",
                "var \$NAME\$ = \$EXPR\$;\$END\$", STMT,
                listOf(JuxPostfixTemplate.Variable("NAME") { it.variableName }),
            ) { true },
            JuxPostfixTemplate("jux.return", "return", "return expr;", "return \$EXPR\$;\$END\$", STMT) { true },
            JuxPostfixTemplate("jux.throw", "throw", "throw expr;", "throw \$EXPR\$;\$END\$", STMT) { true },
            JuxPostfixTemplate("jux.print", "print", "print(expr);", "print(\$EXPR\$);\$END\$", STMT) { true },
            JuxPostfixTemplate("jux.sout", "sout", "print(expr);", "print(\$EXPR\$);\$END\$", STMT) { true },
            JuxPostfixTemplate(
                "jux.switch", "switch", "switch (expr) { case ... }",
                "switch (\$EXPR\$) {\n    \$END\$\n}", STMT,
                dynamicBody = ::switchWithArms,
            ) { true },
            // Inside a generator: hand the value out (§M.2).
            JuxPostfixTemplate("jux.yield", "yield", "yield expr;", "yield \$EXPR\$;\$END\$", STMT) { true },
            JuxPostfixTemplate(
                "jux.try", "try", "try { expr; } catch (Exception e) { }",
                "try {\n    \$EXPR\$;\n} catch (Exception \$E\$) {\n    \$END\$\n}", STMT,
                listOf(JuxPostfixTemplate.Variable("E") { "e" }),
            ) { true },

            // ---- expressions around expressions ----------------------------
            JuxPostfixTemplate("jux.par", "par", "(expr)", "(\$EXPR\$)\$END\$", EXPR) { true },
            JuxPostfixTemplate(
                "jux.cast", "cast", "((Type) expr)",
                "((\$TYPE\$) \$EXPR\$)\$END\$", EXPR,
                listOf(JuxPostfixTemplate.Variable("TYPE") { "Type" }),
            ) { true },
            JuxPostfixTemplate(
                "jux.arg", "arg", "call(expr)",
                "\$CALL\$(\$EXPR\$)\$END\$", EXPR,
                listOf(JuxPostfixTemplate.Variable("CALL") { "call" }),
            ) { true },
            JuxPostfixTemplate("jux.await", "await", "await expr", "await \$EXPR\$\$END\$", EXPR) { true },
            JuxPostfixTemplate("jux.new", "new", "new Type()", "new \$EXPR\$(\$END\$)", EXPR) { it.looksLikeType },
        )
    }
}

/**
 * One postfix template — see [JuxPostfixTemplateProvider].
 *
 * @param body the expansion, with `$EXPR$` for the receiver expression,
 *   `$END$` for the resulting caret, and any [variables] as live variables.
 * @param position where the template may be used: a whole statement, or any
 *   expression.
 * @param applies whether the template fits an expression of this [Shape].
 */
class JuxPostfixTemplate(
    id: String,
    name: String,
    example: String,
    private val body: String,
    private val position: Position,
    private val variables: List<Variable> = emptyList(),
    /**
     * A body worked out from the expression's PSI at expansion time (the
     * document is committed first), or null to fall back to [body].
     */
    private val dynamicBody: ((PsiFile, Int, Int) -> String?)? = null,
    private val applies: (Shape) -> Boolean,
) : PostfixTemplate(id, name, example, null) {

    enum class Position { STATEMENT, EXPRESSION }

    /** A live template variable and how to suggest its value. */
    class Variable(val name: String, val suggest: (Shape) -> String)

    override fun isApplicable(context: PsiElement, copyDocument: Document, newOffset: Int): Boolean {
        val chars = copyDocument.charsSequence
        val start = expressionRangeBefore(chars, newOffset) ?: return false
        if (position == Position.STATEMENT && !isStatementStart(chars, start)) return false
        return applies(Shape.of(chars.subSequence(start, newOffset).toString(), context))
    }

    override fun expand(context: PsiElement, editor: Editor) {
        val document = editor.document
        val caret = editor.caretModel.offset
        val chars = document.charsSequence
        // The platform has already removed `.key` by the time `expand` runs:
        // the expression now ends at the caret. (Looking for the dot here was
        // what made every template delete its key and insert nothing.)
        val start = expressionRangeBefore(chars, caret) ?: return
        val dot = caret
        val expr = chars.subSequence(start, dot).toString()
        val shape = Shape.of(expr, context)

        val chosen = dynamicBody?.let { compute ->
            PsiDocumentManager.getInstance(context.project).commitDocument(document)
            runCatching { compute(context.containingFile, start, caret) }.getOrNull()
        } ?: body

        val indent = lineIndentAt(chars, start)
        // A `$` in the expression (an interpolated string) must not read as a
        // template variable marker.
        val literal = expr.replace("$", "$$")
        val text = chosen
            .replace("\$EXPR_NOT\$", if (shape.isSimple) literal else "($literal)")
            .replace("\$BOUND\$", shape.bound.replace("$", "$$"))
            .replace("\$EXPR\$", literal)
            .replace("\n", "\n$indent")

        val project = context.project
        WriteCommandAction.runWriteCommandAction(project) {
            document.deleteString(start, caret)
            editor.caretModel.moveToOffset(start)
        }
        val manager = TemplateManager.getInstance(project)
        val template = manager.createTemplate("", "", text)
        template.isToReformat = false
        for (v in variables) {
            template.addVariable(v.name, ConstantNode(v.suggest(shape)), true)
        }
        manager.startTemplate(editor, template)
    }

    /**
     * What an expression is, as far as can be told from its text and, for a
     * plain name, from the type written at its declaration.
     *
     * Every question is phrased "may it be ...": an expression that cannot be
     * judged answers yes to all of them.
     */
    class Shape private constructor(
        private val text: String,
        private val typeText: String?,
    ) {
        private val literalBool = text == "true" || text == "false"
        private val literalNumber = NUMBER.matches(text)
        private val literalString = text.startsWith("\"") || text.startsWith("\$\"") || text.startsWith("'")
        private val literalNull = text == "null"
        private val constructed = text.startsWith("new ")
        private val bareType = typeText?.removeSuffix("?")?.substringBefore('<')?.trim()

        /**
         * A single name, call or chain, or an expression already in its own
         * parentheses: needs no (more) parentheses under `!`.
         */
        val isSimple: Boolean = SIMPLE.matches(text) || (text.startsWith("(") && text.endsWith(")"))

        private val knownBool = literalBool || bareType == "bool"
        private val knownNotBool = literalNumber || literalString || literalNull || constructed ||
            (bareType != null && bareType != "bool")

        val maybeBool: Boolean get() = knownBool || !knownNotBool

        val maybeNull: Boolean
            get() = when {
                literalBool || literalNumber || literalString || constructed -> false
                typeText != null -> typeText.endsWith("?")
                else -> true
            }

        private val isArray = typeText?.endsWith("[]") == true || (constructed && text.contains('['))
        private val isCollection = bareType in COLLECTIONS
        private val isInt = literalNumber && !text.contains('.') ||
            bareType in INTEGERS || text.endsWith(".length") || text.endsWith(".len()")

        val maybeIterable: Boolean
            get() = isArray || isCollection ||
                !(literalBool || literalNumber || literalString || literalNull || bareType != null)

        val maybeCountable: Boolean
            get() = isInt || isArray || isCollection ||
                !(literalBool || literalString || literalNull || bareType != null)

        /** A capitalized bare name: the `Type` of `Type.new`. */
        val looksLikeType: Boolean = typeText == null && TYPE_NAME.matches(text)

        /** What a counted loop runs up to: the length of an array or collection, else the value. */
        val bound: String
            get() = when {
                isArray -> "$text.length"
                isCollection -> "$text.len()"
                else -> text
            }

        /** A name for `.var`: the constructed type, the called getter, or the last word. */
        val variableName: String
            get() {
                val base = when {
                    constructed -> text.removePrefix("new ").substringBefore('(').substringBefore('<').substringBefore('[')
                    else -> LAST_WORD.find(text.substringBeforeLast('(').substringBeforeLast('['))?.value ?: "value"
                }
                val trimmed = base.removePrefix("get").ifEmpty { base }
                val name = trimmed.replaceFirstChar { it.lowercaseChar() }
                return if (name.isEmpty() || name in KEYWORDS || literalString || literalNumber) "value" else name
            }

        /** A name for `.for`: the singular of a plural collection name, else `item`. */
        val itemName: String
            get() {
                val word = LAST_WORD.find(text)?.value ?: return "item"
                return when {
                    word.endsWith("ies") && word.length > 3 -> word.dropLast(3) + "y"
                    word.endsWith("s") && word.length > 1 && !word.endsWith("ss") -> word.dropLast(1)
                    else -> "item"
                }
            }

        companion object {
            fun of(text: String, context: PsiElement): Shape {
                val trimmed = text.trim()
                val typeText = if (NAME.matches(trimmed)) {
                    runCatching { JuxTypeInference.writtenTypeTextOf(trimmed, context) }.getOrNull()
                } else {
                    null
                }
                return Shape(trimmed, typeText)
            }

            private val NUMBER = Regex("""-?\d[\d_]*(\.\d+)?[a-zA-Z]*""")
            private val NAME = Regex("""[A-Za-z_]\w*""")
            private val TYPE_NAME = Regex("""[A-Z]\w*""")
            private val SIMPLE = Regex("""[\w.]+(\([^()]*\))?""")
            private val LAST_WORD = Regex("""[A-Za-z_]\w*(?=\W*$)""")
            private val INTEGERS = setOf(
                "int", "uint", "long", "ulong", "short", "ushort", "byte", "ubyte",
                "i8", "u8", "i16", "u16", "i32", "u32", "i64", "u64",
            )
            private val COLLECTIONS = setOf(
                "Vec", "VecDeque", "HashSet", "BTreeSet", "HashMap", "BTreeMap", "Iterable", "Stream",
            )
            private val KEYWORDS = setOf("new", "this", "super", "null", "true", "false")
        }
    }

    companion object {
        /**
         * The largest expression covering exactly `[start, end)` in [file],
         * or null when that range is not one expression.
         */
        fun expressionAt(file: PsiFile, start: Int, end: Int): PsiElement? {
            var e: PsiElement? = file.findElementAt(end - 1) ?: return null
            var best: PsiElement? = null
            while (e != null && e !is PsiFile) {
                val r = e.textRange
                if (r.startOffset < start || r.endOffset > end) break
                if (r.startOffset == start && r.endOffset == end && JuxTypeEngine.isExpression(e)) best = e
                e = e.parent
            }
            return best
        }

        /**
         * The expression scan, exposed for tests.
         *
         * It is the part worth pinning: everything else is document surgery,
         * but getting `f(a, b).c[0].if` to take the whole chain rather than
         * `[0]` is what separates a template that works on real code from one
         * that only works on a bare name.
         */
        @JvmStatic
        fun expressionStartForTest(chars: CharSequence, end: Int): Int? =
            expressionRangeBefore(chars, end)

        /** The `.` that introduced this template's key, scanning back from the caret. */
        fun lastDotBefore(chars: CharSequence, caret: Int): Int? {
            var i = caret
            while (i > 0 && (chars[i - 1].isLetterOrDigit() || chars[i - 1] == '_')) i--
            return if (i > 0 && chars[i - 1] == '.') i - 1 else null
        }

        /**
         * Whether the expression starting at [start] is the start of a
         * statement: only whitespace back to a line break, `{`, `}` or `;`.
         * That is what keeps `x = y.if` and `f(y.return)` from offering
         * statement templates.
         */
        fun isStatementStart(chars: CharSequence, start: Int): Boolean {
            var i = start
            while (i > 0 && (chars[i - 1] == ' ' || chars[i - 1] == '\t')) i--
            if (i == 0) return true
            return chars[i - 1] in "\n\r{};"
        }

        /**
         * Start offset of the expression ending at [end], or null when there is
         * none.
         *
         * Walks back over identifier characters, `.` chains, and BALANCED
         * `()`/`[]` runs, so `f(a, b).c[0]` is taken whole. A string literal is
         * taken whole too. Stops at a statement boundary or an operator, which
         * is what keeps `x = y.if` from swallowing the `x =`. A leading `new `
         * belongs to the expression.
         */
        fun expressionRangeBefore(chars: CharSequence, end: Int): Int? {
            var i = end
            var progressed = false
            while (i > 0) {
                val c = chars[i - 1]
                when {
                    c.isLetterOrDigit() || c == '_' || c == '.' -> {
                        i--
                        progressed = true
                    }

                    c == ')' || c == ']' -> {
                        val open = if (c == ')') '(' else '['
                        var depth = 0
                        while (i > 0) {
                            val d = chars[i - 1]
                            if (d == c) depth++
                            if (d == open) {
                                depth--
                                if (depth == 0) {
                                    i--
                                    break
                                }
                            }
                            i--
                        }
                        progressed = true
                    }

                    c == '"' && !progressed -> {
                        // A string literal: back to its opening quote, and an
                        // interpolation `$` before that.
                        i--
                        while (i > 0 && !(chars[i - 1] == '"' && (i < 2 || chars[i - 2] != '\\'))) i--
                        if (i > 0) i--
                        if (i > 0 && chars[i - 1] == '$') i--
                        progressed = true
                    }

                    else -> break
                }
            }
            if (!progressed || i >= end) return null
            // `new Type(...)` is one expression.
            val before = chars.subSequence(maxOf(0, i - 4), i).toString()
            if (before == "new ") i -= 4
            // A bare keyword is not an expression worth wrapping.
            val text = chars.subSequence(i, end).toString().trim()
            if (text.isEmpty() || text in NON_EXPRESSIONS) return null
            return i
        }

        /** The whitespace prefix of the line containing [offset]. */
        fun lineIndentAt(chars: CharSequence, offset: Int): String {
            var start = offset
            while (start > 0 && chars[start - 1] != '\n') start--
            val sb = StringBuilder()
            var i = start
            while (i < chars.length && (chars[i] == ' ' || chars[i] == '\t')) {
                sb.append(chars[i])
                i++
            }
            return sb.toString()
        }

        /** Words that read as an expression start but are not one. */
        val NON_EXPRESSIONS = setOf(
            "return", "if", "else", "while", "for", "switch", "case", "class",
            "interface", "enum", "record", "public", "private", "protected",
            "static", "final", "const", "var", "void", "new", "import", "package",
        )
    }
}
