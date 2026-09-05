package dev.jux.intellij.templates

import com.intellij.codeInsight.template.postfix.templates.PostfixTemplate
import com.intellij.codeInsight.template.postfix.templates.PostfixTemplateProvider
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile

/**
 * Postfix templates: `expr.if`, `expr.var`, `expr.for`, and friends — type the
 * value first, then say what to do with it.
 *
 * These are the completions that get used most and they were absent entirely.
 * They also matter structurally: the plugin's own completion stands down while
 * `juxc-lsp` is serving, but postfix templates are a separate extension point
 * that keeps working either way, so this is depth the user gets in every
 * configuration.
 *
 * The expression is found by scanning backwards from the `.`, tracking bracket
 * depth, so `xs[0].if` and `f(a, b).var` take the whole expression rather than
 * the last word. That is the same technique the language server uses to find a
 * receiver, and it is why these work on chained calls where the plugin's own
 * member completion still cannot.
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
         * `$EXPR$` is replaced by the expression before the dot and `$END$`
         * marks where the caret lands.
         *
         * Names carry no leading dot: [PostfixTemplate] builds its `key` as
         * `"." + name`, so writing `".if"` here would register `..if`.
         */
        val TEMPLATES: Set<PostfixTemplate> = setOf(
            JuxPostfixTemplate("jux.if", "if", "if (expr) { }", "if (\$EXPR\$) {\n    \$END\$\n}"),
            JuxPostfixTemplate("jux.else", "else", "if (!expr) { }", "if (!(\$EXPR\$)) {\n    \$END\$\n}"),
            JuxPostfixTemplate("jux.not", "not", "!expr", "!(\$EXPR\$)\$END\$"),
            JuxPostfixTemplate("jux.var", "var", "var name = expr;", "var \$END\$ = \$EXPR\$;"),
            JuxPostfixTemplate("jux.for", "for", "for (var it : expr) { }", "for (var it : \$EXPR\$) {\n    \$END\$\n}"),
            JuxPostfixTemplate("jux.while", "while", "while (expr) { }", "while (\$EXPR\$) {\n    \$END\$\n}"),
            JuxPostfixTemplate("jux.return", "return", "return expr;", "return \$EXPR\$;\$END\$"),
            JuxPostfixTemplate("jux.print", "print", "print(expr);", "print(\$EXPR\$);\$END\$"),
            JuxPostfixTemplate("jux.notnull", "notnull", "if (expr != null) { }", "if (\$EXPR\$ != null) {\n    \$END\$\n}"),
            JuxPostfixTemplate("jux.null", "null", "if (expr == null) { }", "if (\$EXPR\$ == null) {\n    \$END\$\n}"),
        )
    }
}

/**
 * One postfix template — see [JuxPostfixTemplateProvider].
 *
 * @param body the expansion, with `$EXPR$` for the receiver expression and
 *   `$END$` for the resulting caret position.
 */
class JuxPostfixTemplate(
    id: String,
    name: String,
    example: String,
    private val body: String,
) : PostfixTemplate(id, name, example, null) {

    override fun isApplicable(context: PsiElement, copyDocument: Document, newOffset: Int): Boolean =
        expressionRangeBefore(copyDocument.charsSequence, newOffset) != null

    override fun expand(context: PsiElement, editor: Editor) {
        val document = editor.document
        val caret = editor.caretModel.offset
        val chars = document.charsSequence
        // The key (`if`, `var`, …) has already been typed and the platform
        // leaves it in the document; everything from the expression's start to
        // the caret is what this template replaces.
        val dot = lastDotBefore(chars, caret) ?: return
        val start = expressionRangeBefore(chars, dot) ?: return
        val expr = chars.subSequence(start, dot).toString()

        val indent = lineIndentAt(chars, start)
        val expanded = body
            .replace("\$EXPR\$", expr)
            .replace("\n", "\n$indent")
        val caretAt = expanded.indexOf("\$END\$")
        val text = expanded.replace("\$END\$", "")

        com.intellij.openapi.command.WriteCommandAction.runWriteCommandAction(context.project) {
            document.replaceString(start, caret, text)
            editor.caretModel.moveToOffset(start + if (caretAt >= 0) caretAt else text.length)
        }
    }

    companion object {
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
         * Start offset of the expression ending at [end], or null when there is
         * none.
         *
         * Walks back over identifier characters, `.` chains, and BALANCED
         * `()`/`[]` runs, so `f(a, b).c[0]` is taken whole. Stops at a
         * statement boundary or an operator, which is what keeps `x = y.if`
         * from swallowing the `x =`.
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

                    else -> break
                }
            }
            if (!progressed || i >= end) return null
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
