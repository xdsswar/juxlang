package dev.jux.intellij.intentions

import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.codeStyle.CodeStyleManager
import com.intellij.psi.impl.source.tree.CompositeElement
import com.intellij.psi.tree.IElementType
import com.intellij.psi.tree.TokenSet
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * The statement and expression shapes the Jux intentions, Join Lines and
 * Unwrap read and rewrite, over the plugin's PSI.
 *
 * The PSI is untyped (every node is a `JuxCompositeElement` with an element
 * type), so these helpers name the parts Java's PSI names as methods:
 * an `if`'s condition and branches, a loop's body, a binary's operands and
 * operator, a block's statements.
 */
object JuxCodeShapes {

    /** Every statement kind: what may stand alone in a block or as a body. */
    val STATEMENTS: TokenSet = TokenSet.create(
        E.LOCAL_VARIABLE, E.EXPRESSION_STATEMENT, E.IF_STATEMENT, E.WHILE_STATEMENT,
        E.DO_WHILE_STATEMENT, E.FOR_STATEMENT, E.FOR_EACH_STATEMENT, E.SWITCH_STATEMENT,
        E.RETURN_STATEMENT, E.BREAK_STATEMENT, E.CONTINUE_STATEMENT, E.THROW_STATEMENT,
        E.TRY_STATEMENT, E.UNSAFE_STATEMENT, E.LABELED_STATEMENT, E.EMPTY_STATEMENT, E.CODE_BLOCK,
    )

    /** The statements that own a body: the ones braces can be added to or removed from. */
    val BODY_OWNERS: TokenSet = TokenSet.create(
        E.IF_STATEMENT, E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT, E.FOR_STATEMENT, E.FOR_EACH_STATEMENT,
    )

    /** True for a composite node (an expression or statement), false for tokens, whitespace and comments. */
    fun isComposite(e: PsiElement?): Boolean = e != null && e.node is CompositeElement

    /** The composite children of [e], in order. */
    fun compositeChildren(e: PsiElement): List<PsiElement> =
        generateSequence(e.firstChild) { it.nextSibling }.filter { isComposite(it) }.toList()

    /** The first composite child after the first child token of type [token], or null. */
    fun compositeAfter(e: PsiElement, token: IElementType): PsiElement? {
        var c = e.firstChild
        while (c != null && c.elementType !== token) c = c.nextSibling
        c = c?.nextSibling
        while (c != null && !isComposite(c)) c = c.nextSibling
        return c
    }

    /** The first direct child token of type [token], or null. */
    fun token(e: PsiElement, token: IElementType): PsiElement? =
        generateSequence(e.firstChild) { it.nextSibling }.firstOrNull { it.elementType === token }

    // ---- if -------------------------------------------------------------

    /** An `if` statement's condition. */
    fun ifCondition(ifStmt: PsiElement): PsiElement? = compositeAfter(ifStmt, T.LPAREN)

    /** An `if` statement's then-branch. */
    fun ifThen(ifStmt: PsiElement): PsiElement? = compositeAfter(ifStmt, T.RPAREN)

    /** An `if` statement's else-branch (itself an `if` for `else if`), or null. */
    fun ifElse(ifStmt: PsiElement): PsiElement? = compositeAfter(ifStmt, T.ELSE_KW)

    /** True when [e] is the else-branch of the `if` that contains it. */
    fun isElseBranch(e: PsiElement): Boolean {
        val parent = e.parent ?: return false
        return parent.elementType === E.IF_STATEMENT && ifElse(parent) == e
    }

    // ---- loops ----------------------------------------------------------

    /** The body of a loop (`while`, `for`, for-each or `do`). */
    fun loopBody(loop: PsiElement): PsiElement? = when (loop.elementType) {
        E.DO_WHILE_STATEMENT -> compositeAfter(loop, T.DO_KW)
        E.WHILE_STATEMENT, E.FOR_STATEMENT, E.FOR_EACH_STATEMENT ->
            // The body follows the header's closing parenthesis. `for` holds a
            // `;`-separated header whose parts are composite too, so walk to the
            // LAST `)` at depth one: the header's own.
            compositeChildren(loop).lastOrNull()?.takeIf { it.textRange.startOffset > (headerEnd(loop) ?: -1) }
        else -> null
    }

    private fun headerEnd(loop: PsiElement): Int? =
        generateSequence(loop.firstChild) { it.nextSibling }.lastOrNull { it.elementType === T.RPAREN }?.textRange?.startOffset

    /** The keyword that opens [stmt], as the intentions name it: `if`, `while`, `for`, `do`. */
    fun keyword(stmt: PsiElement): String = when (stmt.elementType) {
        E.IF_STATEMENT -> "if"
        E.WHILE_STATEMENT -> "while"
        E.DO_WHILE_STATEMENT -> "do"
        else -> "for"
    }

    // ---- blocks ---------------------------------------------------------

    /** The children of a `{ }` block between its braces, whitespace excluded, comments included. */
    fun blockContent(block: PsiElement): List<PsiElement> =
        generateSequence(block.firstChild) { it.nextSibling }
            .filter { it !is PsiWhiteSpace && it.elementType !== T.LBRACE && it.elementType !== T.RBRACE }
            .toList()

    /** The one statement a block holds, when it holds exactly one and no comment. */
    fun singleStatement(block: PsiElement): PsiElement? {
        if (block.elementType !== E.CODE_BLOCK) return block
        val content = blockContent(block)
        if (content.size != 1 || content[0] is PsiComment) return null
        return content[0].takeIf { it.elementType in STATEMENTS }
    }

    /** A body's text as a braced block: itself when it already is one. */
    fun asBlockText(body: PsiElement): String =
        if (body.elementType === E.CODE_BLOCK) body.text else "{\n${body.text}\n}"

    // ---- binary ---------------------------------------------------------

    /** A binary expression's operator token. */
    fun operator(binary: PsiElement): PsiElement? =
        generateSequence(binary.firstChild) { it.nextSibling }
            .firstOrNull { !isComposite(it) && it !is PsiWhiteSpace && it !is PsiComment }

    /** A binary expression's left operand. */
    fun left(binary: PsiElement): PsiElement? = compositeChildren(binary).firstOrNull()

    /** A binary expression's right operand. */
    fun right(binary: PsiElement): PsiElement? = compositeChildren(binary).drop(1).lastOrNull()

    /** True when [e] is a binary whose operator is [op]. */
    fun isBinary(e: PsiElement?, op: IElementType): Boolean =
        e != null && e.elementType === E.BINARY_EXPRESSION && operator(e)?.elementType === op

    /** Binding strength of a binary operator, higher binds tighter (the parser's §A.4 ladder). */
    fun precedence(op: IElementType?): Int = when (op) {
        T.QUESTION_COLON, T.QUESTION_QUESTION -> 1
        T.OR_OR -> 2
        T.AND_AND -> 3
        T.PIPE -> 4
        T.CARET -> 5
        T.AMP -> 6
        T.EQ_EQ, T.NOT_EQ, T.STRICT_EQ, T.STRICT_NOT_EQ -> 7
        T.LT, T.LE, T.GT, T.GE, T.FAT_ARROW -> 8
        T.LT_LT, T.GT_GT, T.LT_LT_PERCENT, T.GT_GT_PERCENT -> 10
        T.PLUS, T.MINUS, T.PLUS_PERCENT, T.MINUS_PERCENT -> 12
        T.STAR, T.SLASH, T.PERCENT, T.STAR_PERCENT -> 13
        else -> 99
    }

    /** [e]'s text with one layer of parentheses removed. */
    fun unparenthesized(e: PsiElement): PsiElement {
        var cur = e
        while (cur.elementType === E.PARENTHESIZED_EXPRESSION) {
            cur = compositeChildren(cur).firstOrNull() ?: break
        }
        return cur
    }

    /**
     * True when [e] must be parenthesized to stand as an operand of an
     * operator with binding strength [outer]: an assignment, a `?:`, a lambda,
     * or a binary that binds looser.
     */
    fun needsParens(e: PsiElement, outer: Int): Boolean = when (e.elementType) {
        E.ASSIGNMENT_EXPRESSION, E.CONDITIONAL_EXPRESSION, E.LAMBDA_EXPRESSION -> true
        E.BINARY_EXPRESSION -> precedence(operator(e)?.elementType) < outer
        else -> false
    }

    /** [e]'s text, parenthesized when it needs to be at binding strength [outer]. */
    fun operandText(e: PsiElement, outer: Int): String = if (needsParens(e, outer)) "(${e.text})" else e.text

    // ---- negation -------------------------------------------------------

    private val NEGATED = mapOf(
        T.EQ_EQ to "!=", T.NOT_EQ to "==", T.STRICT_EQ to "!==", T.STRICT_NOT_EQ to "===",
        T.LT to ">=", T.GE to "<", T.GT to "<=", T.LE to ">",
    )

    /**
     * The text of `!e`, written the way a person would: `a != b` for
     * `a == b`, `x` for `!x`, `false` for `true`, and `!(...)` only where the
     * operand needs the parentheses.
     */
    fun negate(e: PsiElement): String {
        val bare = unparenthesized(e)
        when (bare.elementType) {
            E.UNARY_EXPRESSION -> if (bare.firstChild?.elementType === T.BANG) {
                val operand = compositeChildren(bare).firstOrNull()
                if (operand != null) return unparenthesized(operand).text
            }
            E.BINARY_EXPRESSION -> {
                val op = operator(bare)
                val flipped = NEGATED[op?.elementType]
                val l = left(bare)
                val r = right(bare)
                if (flipped != null && l != null && r != null) return "${l.text} $flipped ${r.text}"
            }
            E.LITERAL_EXPRESSION -> when (bare.text) {
                "true" -> return "false"
                "false" -> return "true"
            }
        }
        return when (bare.elementType) {
            E.REFERENCE_EXPRESSION, E.CALL_EXPRESSION, E.FIELD_ACCESS_EXPRESSION, E.INDEX_EXPRESSION,
            E.LITERAL_EXPRESSION, E.THIS_EXPRESSION, E.PARENTHESIZED_EXPRESSION -> "!${bare.text}"
            else -> "!(${bare.text})"
        }
    }

    // ---- editing --------------------------------------------------------

    /**
     * Replace [range] of [context]'s file with [text], commit, and reformat
     * what was written. Returns the range the new text occupies afterwards.
     *
     * The rewrites build source text rather than PSI: the Jux PSI has no
     * element factory for statements, and a reparse of the written text is
     * exactly what the user would get by typing it.
     */
    fun replace(context: PsiElement, range: TextRange, text: String): TextRange {
        val file = context.containingFile
        val project = file.project
        val manager = PsiDocumentManager.getInstance(project)
        val document = file.viewProvider.document ?: manager.getDocument(file) ?: return range
        manager.doPostponedOperationsAndUnblockDocument(document)
        document.replaceString(range.startOffset, range.endOffset, text)
        val marker = document.createRangeMarker(range.startOffset, range.startOffset + text.length)
        manager.commitDocument(document)
        CodeStyleManager.getInstance(project).reformatText(file, marker.startOffset, marker.endOffset)
        manager.doPostponedOperationsAndUnblockDocument(document)
        return TextRange(marker.startOffset, marker.endOffset).also { marker.dispose() }
    }

    /** [replace] over [element]'s own range. */
    fun replace(element: PsiElement, text: String): TextRange = replace(element, element.textRange, text)
}
