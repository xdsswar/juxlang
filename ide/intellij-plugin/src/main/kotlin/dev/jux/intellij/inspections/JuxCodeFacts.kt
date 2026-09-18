package dev.jux.intellij.inspections

import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.codeStyle.CodeStyleManager
import com.intellij.psi.tree.IElementType
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * Facts about Jux code that several inspections and quick fixes need, read
 * straight off the PSI: a statement's condition and branches, whether an
 * expression is free of side effects, whether a statement can finish without
 * jumping away, and how to splice one statement's contents in place of
 * another.
 *
 * Everything here answers conservatively. An inspection built on it flags
 * code only when the answer is certain; "don't know" always means "don't
 * report", because a warning on correct code teaches the user to ignore the
 * editor.
 */
object JuxCodeFacts {

    /** Element types that are executable statements inside a code block. */
    val STATEMENT_TYPES: Set<IElementType> = setOf(
        E.EXPRESSION_STATEMENT, E.IF_STATEMENT, E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT,
        E.FOR_STATEMENT, E.FOR_EACH_STATEMENT, E.SWITCH_STATEMENT, E.RETURN_STATEMENT,
        E.BREAK_STATEMENT, E.CONTINUE_STATEMENT, E.THROW_STATEMENT, E.TRY_STATEMENT,
        E.UNSAFE_STATEMENT, E.LABELED_STATEMENT, E.EMPTY_STATEMENT, E.CODE_BLOCK,
        E.LOCAL_VARIABLE,
    )

    /** Statements that always leave the current flow: nothing after them runs. */
    val JUMP_TYPES: Set<IElementType> = setOf(
        E.RETURN_STATEMENT, E.THROW_STATEMENT, E.BREAK_STATEMENT, E.CONTINUE_STATEMENT,
    )

    fun isStatement(e: PsiElement?): Boolean = e != null && e.elementType in STATEMENT_TYPES

    // ------------------------------------------------------------ structure

    /** The parenthesized condition of an `if` / `while` / `do … while`. */
    fun conditionOf(stmt: PsiElement): PsiElement? {
        var sawParen = false
        var c = stmt.firstChild
        while (c != null) {
            if (c.elementType === T.LPAREN) sawParen = true
            else if (sawParen && JuxTypeEngine.isExpression(c)) return c
            c = c.nextSibling
        }
        return null
    }

    /** The statement an `if` runs when its condition holds. */
    fun thenBranch(ifStmt: PsiElement): PsiElement? {
        var sawParen = false
        var c = ifStmt.firstChild
        while (c != null) {
            if (c.elementType === T.RPAREN) sawParen = true
            else if (sawParen && isStatement(c)) return c
            if (c.elementType === T.ELSE_KW) return null
            c = c.nextSibling
        }
        return null
    }

    /** The statement after `else`, or null for an `if` without one. */
    fun elseBranch(ifStmt: PsiElement): PsiElement? {
        var sawElse = false
        var c = ifStmt.firstChild
        while (c != null) {
            if (c.elementType === T.ELSE_KW) sawElse = true
            else if (sawElse && isStatement(c)) return c
            c = c.nextSibling
        }
        return null
    }

    /** The `else` keyword of an `if`, or null. */
    fun elseKeyword(ifStmt: PsiElement): PsiElement? =
        ifStmt.node.findChildByType(T.ELSE_KW)?.psi

    /** The body statement of a `while` / `for` / `for-each` loop (after its `)`). */
    fun loopBody(loop: PsiElement): PsiElement? {
        var depth = 0
        var sawParen = false
        var c = loop.firstChild
        while (c != null) {
            when (c.elementType) {
                T.LPAREN -> depth++
                T.RPAREN -> { depth--; if (depth == 0) sawParen = true }
                else -> if (sawParen && depth == 0 && isStatement(c)) return c
            }
            c = c.nextSibling
        }
        return null
    }

    /** The statements directly inside a code block, in order. */
    fun statementsOf(block: PsiElement): List<PsiElement> = block.children.filter { isStatement(it) }

    /**
     * Every child of [e], leaves included. `getChildren()` on AST-backed PSI
     * returns only composite children, so comments and tokens never show up
     * in it; anything that must see a comment walks siblings instead.
     */
    fun allChildren(e: PsiElement): List<PsiElement> = generateSequence(e.firstChild) { it.nextSibling }.toList()

    /** True when a code block holds a comment directly between its braces. */
    fun hasComment(block: PsiElement): Boolean = allChildren(block).any { it is PsiComment }

    /**
     * What a code block holds between its braces (statements and comments),
     * without the braces and the whitespace around them.
     */
    fun blockContents(block: PsiElement): List<PsiElement> = allChildren(block).filter {
        it.elementType !== T.LBRACE && it.elementType !== T.RBRACE && it !is PsiWhiteSpace
    }

    // ------------------------------------------------------------ text edits
    //
    // Multi-line edits go through the document, not PSI: inserting a
    // whitespace leaf with `addAfter` lets the platform merge it into its
    // neighbor, and the next insertion then lands somewhere else entirely.
    // Text plus a range marker plus the formatter is how the platform's own
    // fixes stay put.

    /**
     * Replaces `[start, end)` of [file] with [text], reformats what was
     * written, and returns the range it covers afterwards.
     */
    fun edit(project: Project, file: PsiFile, start: Int, end: Int, text: String): TextRange {
        val pdm = PsiDocumentManager.getInstance(project)
        val doc = pdm.getDocument(file) ?: error("no document for ${file.name}")
        pdm.doPostponedOperationsAndUnblockDocument(doc)
        doc.replaceString(start, end, text)
        val marker = doc.createRangeMarker(start, start + text.length)
        pdm.commitDocument(doc)
        if (text.isNotBlank()) CodeStyleManager.getInstance(project).reformatText(file, marker.startOffset, marker.endOffset)
        pdm.doPostponedOperationsAndUnblockDocument(doc)
        pdm.commitDocument(doc)
        val range = TextRange(marker.startOffset, marker.endOffset)
        marker.dispose()
        return range
    }

    /**
     * The largest element that starts where [range] starts (after any
     * whitespace) and ends inside it: the statement or member just written.
     */
    fun elementIn(file: PsiFile, range: TextRange): PsiElement? {
        var e = file.findElementAt(range.startOffset) ?: return null
        while (e is PsiWhiteSpace) e = PsiTreeUtil.nextLeaf(e) ?: return null
        while (true) {
            val parent = e.parent ?: break
            if (parent is PsiFile) break
            if (parent.textRange.startOffset != e.textRange.startOffset || parent.textRange.endOffset > range.endOffset) break
            e = parent
        }
        return e
    }

    /** Deletes [stmt] with the line break and indentation in front of it. */
    fun deleteLine(project: Project, stmt: PsiElement) {
        val file = stmt.containingFile
        var prev = stmt.prevSibling
        while (prev is PsiWhiteSpace) prev = prev.prevSibling
        val start = prev?.textRange?.endOffset ?: stmt.textRange.startOffset
        edit(project, file, start, stmt.textRange.endOffset, "")
    }

    /** Writes [lines] after [anchor], each on its own line. Returns the range written. */
    fun insertLinesAfter(project: Project, anchor: PsiElement, lines: List<String>): TextRange {
        val end = anchor.textRange.endOffset
        return edit(project, anchor.containingFile, end, end, lines.joinToString("") { "\n$it" })
    }

    /** Adds [stmtText] as the last statement of [block], on its own line. Returns the new statement. */
    fun addStatementAtEnd(project: Project, block: PsiElement, stmtText: String): PsiElement? {
        val close = block.lastChild?.takeIf { it.elementType === T.RBRACE } ?: return null
        var last = close.prevSibling
        while (last is PsiWhiteSpace) last = last.prevSibling
        val at = (last ?: block.firstChild).textRange.endOffset
        val file = block.containingFile
        // The closing brace keeps a line of its own.
        val closeOnOwnLine = close.prevSibling is PsiWhiteSpace && close.prevSibling.text.contains('\n')
        val range = edit(project, file, at, at, "\n$stmtText" + if (closeOnOwnLine) "" else "\n")
        return elementIn(file, range)
    }

    /** The operator token of a binary expression: the first child that is not an operand. */
    fun binaryOperator(binary: PsiElement): PsiElement? {
        var c = binary.firstChild
        while (c != null) {
            if (c !is PsiWhiteSpace && c !is PsiComment && !JuxTypeEngine.isExpression(c)) return c
            c = c.nextSibling
        }
        return null
    }

    /** The operands of a binary expression, left then right. */
    fun operands(binary: PsiElement): List<PsiElement> = JuxTypeEngine.expressionChildren(binary)

    /** [e] with any wrapping parentheses removed. */
    fun stripParens(e: PsiElement?): PsiElement? {
        var cur = e
        while (cur != null && cur.elementType === E.PARENTHESIZED_EXPRESSION) {
            cur = JuxTypeEngine.firstExpressionChild(cur)
        }
        return cur
    }

    /** `true` / `false` when [e] is (a parenthesized) boolean literal, else null. */
    fun boolLiteral(e: PsiElement?): Boolean? {
        val s = stripParens(e) ?: return null
        if (s.elementType !== E.LITERAL_EXPRESSION) return null
        val tok = s.firstChild ?: return null
        if (tok.elementType !== T.BOOL_LITERAL) return null
        return tok.text == "true"
    }

    /** The literal token of a (parenthesized) literal expression, or null. */
    fun literalToken(e: PsiElement?): PsiElement? {
        val s = stripParens(e) ?: return null
        if (s.elementType !== E.LITERAL_EXPRESSION) return null
        return s.firstChild
    }

    // ------------------------------------------------------------ exact types

    /** A plain type name: no pointer, array, nullable, generic or qualified form. */
    fun isSimpleTypeText(text: String): Boolean =
        text.trim().let { t -> t.isNotEmpty() && t.all { it.isLetterOrDigit() || it == '_' } }

    /** Declarations whose written type is the value's type. */
    private val VALUE_DECLARATIONS: Set<IElementType> = setOf(E.LOCAL_VARIABLE, E.PARAMETER, E.FIELD_DECLARATION)

    /**
     * [e]'s type when it is known EXACTLY, else null: a literal with no
     * suffix (`5` is an `int`, `1.5` a `double`), a name declared with a
     * written simple type, or a cast. The general type engine infers more,
     * but it reads `5L` as an `int` and `var x = 5L` along with it; a check
     * built on that would call a narrowing cast redundant.
     */
    fun exactType(e: PsiElement): JuxType? {
        val s = stripParens(e) ?: return null
        return when (s.elementType) {
            E.LITERAL_EXPRESSION -> {
                val tok = s.firstChild ?: return null
                when (tok.elementType) {
                    T.INT_LITERAL -> if (tok.text.all { it.isDigit() || it == '_' }) JuxType.Primitive("int") else null
                    T.FLOAT_LITERAL ->
                        if (tok.text.all { it.isDigit() || it == '_' || it == '.' }) JuxType.Primitive("double") else null
                    T.BOOL_LITERAL -> JuxType.Primitive("bool")
                    T.CHAR_LITERAL -> JuxType.Primitive("char")
                    else -> null
                }
            }
            E.REFERENCE_EXPRESSION -> {
                val target = JuxTypeEngine.resolveReferenceExpression(s) ?: return null
                if (target.elementType !in VALUE_DECLARATIONS) return null
                val ref = target.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return null
                if (!isSimpleTypeText(ref.text)) return null
                JuxTypeEngine.typeOfTypeReference(ref)
            }
            E.CAST_EXPRESSION -> s.node.findChildByType(E.TYPE_REFERENCE)?.psi
                ?.takeIf { isSimpleTypeText(it.text) }
                ?.let { JuxTypeEngine.typeOfTypeReference(it) }
            else -> null
        }
    }

    /** Two exactly-known types that are the same simple type (same primitive, same non-generic class). */
    fun sameSimpleType(a: JuxType, b: JuxType): Boolean = when {
        a is JuxType.Primitive && b is JuxType.Primitive -> a.name == b.name
        a is JuxType.ClassType && b is JuxType.ClassType ->
            a.decl == b.decl && a.args.isEmpty() && b.args.isEmpty() && a.decl.name != null
        else -> false
    }

    /** Primitive types whose `==` is reflexive: every primitive but the floats (`NaN != NaN`). */
    fun isReflexivePrimitive(t: JuxType?): Boolean =
        t is JuxType.Primitive && t.name !in FLOAT_NAMES && t.name != "String" && t.name != "string"

    private val FLOAT_NAMES = setOf("float", "double", "f32", "f64")

    // ------------------------------------------------------------ side effects

    /**
     * True when evaluating [e] cannot change anything: no call, no `new`, no
     * assignment, no `++` / `--`, no `await`. A call may be pure, but the IDE
     * cannot tell, so a call is always treated as a side effect.
     */
    fun isSideEffectFree(e: PsiElement?): Boolean {
        if (e == null) return false
        var pure = true
        fun visit(x: PsiElement) {
            if (!pure) return
            when (x.elementType) {
                E.CALL_EXPRESSION, E.NEW_EXPRESSION, E.ASSIGNMENT_EXPRESSION, E.LAMBDA_EXPRESSION,
                E.SWITCH_EXPRESSION -> { pure = false; return }
                T.PLUS_PLUS, T.MINUS_MINUS, T.AWAIT_KW -> { pure = false; return }
            }
            var c = x.firstChild
            while (c != null) { visit(c); c = c.nextSibling }
        }
        visit(e)
        return pure
    }

    /**
     * True when [e] is a plain value read: a name, `this`, or a chain of
     * member reads on one (`this.x`, `a.b.c`). Two such expressions with the
     * same text read the same place.
     */
    fun isPlainRead(e: PsiElement?): Boolean {
        val s = stripParens(e) ?: return false
        return when (s.elementType) {
            E.REFERENCE_EXPRESSION, E.THIS_EXPRESSION -> true
            E.FIELD_ACCESS_EXPRESSION -> {
                // `a?.b` reads maybe-nothing; keep it out.
                s.node.findChildByType(T.QUESTION_DOT) == null &&
                    isPlainRead(JuxTypeEngine.firstExpressionChild(s))
            }
            else -> false
        }
    }

    /** [e]'s text with all whitespace and comments removed, for structural comparison. */
    fun normalizedText(e: PsiElement): String {
        val sb = StringBuilder()
        fun visit(x: PsiElement) {
            if (x is PsiWhiteSpace || x is PsiComment) return
            if (x.firstChild == null) sb.append(x.text) else {
                var c = x.firstChild
                while (c != null) { visit(c); c = c.nextSibling }
            }
        }
        visit(e)
        return sb.toString()
    }

    // ------------------------------------------------------------ flow

    /**
     * Whether [stmt] can finish and let the next statement run (JLS 14.22's
     * "can complete normally", conservatively). The answer errs toward
     * "cannot" wherever the IDE is unsure (a `switch`, a labeled statement,
     * a loop with a `break` somewhere inside), because the one caller that
     * matters, "missing return", must never flag a method that does return.
     */
    fun canCompleteNormally(stmt: PsiElement): Boolean = when (stmt.elementType) {
        E.RETURN_STATEMENT, E.THROW_STATEMENT, E.BREAK_STATEMENT, E.CONTINUE_STATEMENT -> false
        E.CODE_BLOCK, E.UNSAFE_STATEMENT -> {
            val block = if (stmt.elementType === E.CODE_BLOCK) stmt
            else stmt.node.findChildByType(E.CODE_BLOCK)?.psi
            block == null || statementsOf(block).all { canCompleteNormally(it) }
        }
        E.IF_STATEMENT -> {
            val then = thenBranch(stmt)
            val otherwise = elseBranch(stmt)
            when (boolLiteral(conditionOf(stmt))) {
                // `if (true) return;` never takes the else path.
                true -> then == null || canCompleteNormally(then)
                else -> otherwise == null ||
                    (then == null || canCompleteNormally(then)) || canCompleteNormally(otherwise)
            }
        }
        // `while (true)` / `for (;;)` / `do … while (true)` without a `break`
        // run forever; any other loop can end.
        E.WHILE_STATEMENT -> boolLiteral(conditionOf(stmt)) != true || breaksOut(stmt)
        E.FOR_STATEMENT -> !forHasNoCondition(stmt) || breaksOut(stmt)
        // A `do` body runs at least once: when it always jumps away (and no
        // `continue` reaches the condition), the loop never finishes.
        E.DO_WHILE_STATEMENT -> {
            val body = stmt.children.firstOrNull { isStatement(it) }
            val reachesCondition = body == null || canCompleteNormally(body) || continuesIn(stmt)
            (reachesCondition && boolLiteral(conditionOf(stmt)) != true) || breaksOut(stmt)
        }
        E.TRY_STATEMENT -> tryCanCompleteNormally(stmt)
        // A switch or a labeled statement needs exhaustiveness and label
        // analysis the IDE does not do: assume it may jump away.
        E.SWITCH_STATEMENT, E.LABELED_STATEMENT -> false
        else -> true
    }

    /**
     * The opposite question, answered in the opposite direction: true only
     * when [stmt] CERTAINLY never lets the next statement run (it returns,
     * throws, breaks or continues on every path the IDE can see). Used where
     * a wrong "yes" would change behavior, such as removing an `else`.
     */
    fun definitelyJumps(stmt: PsiElement): Boolean = when (stmt.elementType) {
        E.RETURN_STATEMENT, E.THROW_STATEMENT, E.BREAK_STATEMENT, E.CONTINUE_STATEMENT -> true
        E.CODE_BLOCK -> statementsOf(stmt).any { definitelyJumps(it) }
        E.IF_STATEMENT -> {
            val then = thenBranch(stmt)
            val otherwise = elseBranch(stmt)
            then != null && otherwise != null && definitelyJumps(then) && definitelyJumps(otherwise)
        }
        else -> false
    }

    private fun tryCanCompleteNormally(tryStmt: PsiElement): Boolean {
        val finally = tryStmt.node.findChildByType(E.FINALLY_CLAUSE)?.psi
        val finallyBlock = finally?.node?.findChildByType(E.CODE_BLOCK)?.psi
        if (finallyBlock != null && !canCompleteNormally(finallyBlock)) return false
        val body = tryStmt.node.findChildByType(E.CODE_BLOCK)?.psi ?: return false
        if (canCompleteNormally(body)) return true
        return tryStmt.children
            .filter { it.elementType === E.CATCH_CLAUSE }
            .any { c -> c.node.findChildByType(E.CODE_BLOCK)?.psi?.let { canCompleteNormally(it) } ?: true }
    }

    /** `for (init; ; update)`: two semicolons with nothing between them. */
    private fun forHasNoCondition(forStmt: PsiElement): Boolean {
        var semis = 0
        var c = forStmt.firstChild
        while (c != null) {
            if (c.elementType === T.SEMICOLON) {
                semis++
                if (semis == 1) {
                    var n = c.nextSibling
                    while (n is PsiWhiteSpace || n is PsiComment) n = n.nextSibling
                    return n?.elementType === T.SEMICOLON
                }
            }
            if (c.elementType === T.RPAREN) return false
            c = c.nextSibling
        }
        return false
    }

    /**
     * True when an unlabeled `break` inside [loop] leaves it: one that is not
     * inside a nested loop, `switch` or lambda (those take the `break` for
     * themselves). A labeled `break` is not counted; not counting it can only
     * make the loop look endless, which the callers read as "don't report".
     */
    private fun breaksOut(loop: PsiElement): Boolean = jumpsTo(loop, E.BREAK_STATEMENT, stopAtSwitch = true)

    /** The same for `continue`, which a `switch` does not capture. */
    private fun continuesIn(loop: PsiElement): Boolean = jumpsTo(loop, E.CONTINUE_STATEMENT, stopAtSwitch = false)

    private fun jumpsTo(loop: PsiElement, jump: IElementType, stopAtSwitch: Boolean): Boolean {
        var found = false
        fun visit(x: PsiElement) {
            if (found) return
            if (x !== loop) {
                val t = x.elementType
                if (t in LOOP_TYPES || t === E.LAMBDA_EXPRESSION) return
                if (stopAtSwitch && (t === E.SWITCH_STATEMENT || t === E.SWITCH_EXPRESSION)) return
                if (t === jump) {
                    // Only an unlabeled jump: `break;`, never `break outer;`.
                    if (x.node.findChildByType(T.IDENTIFIER) == null) found = true
                    return
                }
            }
            var c = x.firstChild
            while (c != null) { visit(c); c = c.nextSibling }
        }
        visit(loop)
        return found
    }

    private val LOOP_TYPES: Set<IElementType> = setOf(
        E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT, E.FOR_STATEMENT, E.FOR_EACH_STATEMENT,
    )

    // ------------------------------------------------------------ editing

    /**
     * Replace [stmt] with what [branch] runs: a code block's own statements
     * (and comments) when [stmt] sits directly in a block, the branch itself
     * otherwise (as the body of an unbraced `if`, where one statement must
     * stay one statement). A null [branch] deletes [stmt].
     */
    fun replaceWithBranch(project: Project, stmt: PsiElement, branch: PsiElement?) {
        val parent = stmt.parent
        if (branch == null) {
            if (parent?.elementType === E.CODE_BLOCK) deleteLine(project, stmt)
            else stmt.replace(JuxElementFactory.createCodeBlock(project, ""))
            return
        }
        if (parent?.elementType !== E.CODE_BLOCK || branch.elementType !== E.CODE_BLOCK) {
            reformat(project, stmt.replace(branch.copy()))
            return
        }
        val inner = blockContents(branch)
        if (inner.isEmpty()) {
            deleteLine(project, stmt)
            return
        }
        // The block's own text between its first and last item, as written.
        val text = stmt.containingFile.text.substring(inner.first().textRange.startOffset, inner.last().textRange.endOffset)
        edit(project, stmt.containingFile, stmt.textRange.startOffset, stmt.textRange.endOffset, text)
    }

    /** Reformat [e] with the Jux formatter, if it is still in a file. */
    fun reformat(project: Project, e: PsiElement) {
        if (e.isValid) CodeStyleManager.getInstance(project).reformat(e)
    }

    /**
     * The default value a Jux variable of type [typeText] starts from, or null
     * when the type has none the IDE can name (a class with no known
     * constructor): `0`, `0.0`, `false`, `'\0'`, `""`, and `null` for `T?`.
     */
    fun defaultValue(typeText: String): String? {
        val t = typeText.trim()
        if (t.endsWith("?")) return "null"
        return when (t) {
            "int", "uint", "long", "ulong", "short", "ushort", "byte", "ubyte",
            "i8", "u8", "i16", "u16", "i32", "u32", "i64", "u64", "isize", "usize" -> "0"
            "float", "double", "f32", "f64" -> "0.0"
            "bool" -> "false"
            "char" -> "'\\0'"
            "String", "string" -> "\"\""
            else -> null
        }
    }
}
