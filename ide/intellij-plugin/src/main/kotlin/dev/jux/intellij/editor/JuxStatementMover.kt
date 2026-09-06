package dev.jux.intellij.editor

import com.intellij.codeInsight.editorActions.moveUpDown.LineRange
import com.intellij.codeInsight.editorActions.moveUpDown.StatementUpDownMover
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile

/**
 * Move Statement Up / Down (`Ctrl+Shift+↑` / `Ctrl+Shift+↓`) — moves whole
 * statements inside a block, and whole member declarations inside a type body.
 *
 * The unit that moves is always a complete declaration or statement, never a
 * line: moving line-wise through a multi-line `if` would tear its header off its
 * body. And when there is no sibling to swap with, the move is **refused**
 * rather than allowed to escape the enclosing block — a held-down shortcut must
 * not be able to lift a statement out of the method it belongs to.
 */
class JuxStatementMover : StatementUpDownMover() {

    override fun checkAvailable(editor: Editor, file: PsiFile, info: MoveInfo, down: Boolean): Boolean {
        if (file !is JuxFile) return false
        val document = editor.document

        val (first, last) = selectedRange(file, editor) ?: return false
        info.toMove = LineRange(lineOf(document, first.textRange.startOffset), lineOf(document, last.textRange.endOffset) + 1)

        val target = sibling(if (down) last else first, down)
        if (target == null) {
            // No sibling to swap with: refuse rather than let the platform fall
            // back to shifting raw lines, which would lift the statement out of
            // the block it belongs to.
            return info.prohibitMove()
        }
        info.toMove2 = LineRange(
            lineOf(document, target.textRange.startOffset),
            lineOf(document, target.textRange.endOffset) + 1,
        )
        return true
    }

    /**
     * The movable elements the caret or selection covers, as (first, last).
     *
     * With no selection this is the single statement the caret sits in. With a
     * selection it is every movable sibling the selection touches, so a
     * multi-statement block moves as one piece.
     */
    private fun selectedRange(file: PsiFile, editor: Editor): Pair<PsiElement, PsiElement>? {
        val selection = editor.selectionModel
        val start = if (selection.hasSelection()) selection.selectionStart else editor.caretModel.offset
        val end = if (selection.hasSelection()) selection.selectionEnd else start

        val first = movableAt(file, start) ?: return null
        if (end <= first.textRange.endOffset) return first to first

        var last = first
        var next = sibling(last, down = true)
        while (next != null && next.textRange.startOffset < end) {
            last = next
            next = sibling(last, down = true)
        }
        return first to last
    }

    /**
     * The nearest enclosing movable element at [offset] — a statement whose
     * parent is a block or the file, or a member whose parent is a class body.
     *
     * `offset - 1` is probed too, so the shortcut still works with the caret
     * resting at the very end of a line.
     */
    private fun movableAt(file: PsiFile, offset: Int): PsiElement? {
        val at = file.findElementAt(offset)
        val before = file.findElementAt((offset - 1).coerceAtLeast(0))
        // At the end of a line `findElementAt` returns the FOLLOWING whitespace,
        // whose ancestor chain skips the statement entirely and lands on the
        // enclosing method -- so a caret after `var a = 1;` moved the whole
        // method. The token before the caret is the one the user is on.
        val primary = if (at == null || at is PsiWhiteSpace) before else at
        return movableFrom(primary) ?: movableFrom(if (primary === at) before else at)
    }

    private fun movableFrom(from: PsiElement?): PsiElement? {
        var e = from
        while (e != null && e !is PsiFile) {
            val type = e.node?.elementType
            val parentType = e.parent?.node?.elementType
            val inBlock = parentType === E.CODE_BLOCK || e.parent is PsiFile
            if (type in MOVABLE_STATEMENTS && inBlock) return e
            if (type in MOVABLE_MEMBERS && parentType === E.CLASS_BODY) return e
            e = e.parent
        }
        return null
    }

    /** The previous or next movable sibling, skipping whitespace and comments. */
    private fun sibling(from: PsiElement, down: Boolean): PsiElement? {
        var e: PsiElement? = if (down) from.nextSibling else from.prevSibling
        while (e != null) {
            if (e !is PsiWhiteSpace && e !is PsiComment) {
                val type = e.node?.elementType
                if (type in MOVABLE_STATEMENTS || type in MOVABLE_MEMBERS) return e
                // Anything else at this level (a stray brace, an error node) is
                // a boundary: swapping across it would move the statement out.
                return null
            }
            e = if (down) e.nextSibling else e.prevSibling
        }
        return null
    }

    private fun lineOf(document: Document, offset: Int): Int =
        document.getLineNumber(offset.coerceIn(0, document.textLength))

    private companion object {
        /** Statement kinds that move as a unit inside a block. */
        val MOVABLE_STATEMENTS = setOf(
            E.LOCAL_VARIABLE,
            E.EXPRESSION_STATEMENT,
            E.IF_STATEMENT,
            E.WHILE_STATEMENT,
            E.DO_WHILE_STATEMENT,
            E.FOR_STATEMENT,
            E.FOR_EACH_STATEMENT,
            E.SWITCH_STATEMENT,
            E.RETURN_STATEMENT,
            E.BREAK_STATEMENT,
            E.CONTINUE_STATEMENT,
            E.THROW_STATEMENT,
            E.TRY_STATEMENT,
            E.UNSAFE_STATEMENT,
            E.LABELED_STATEMENT,
            E.EMPTY_STATEMENT,
            E.CODE_BLOCK,
        )

        /** Member kinds that move as a unit inside a type body. */
        val MOVABLE_MEMBERS = setOf(
            E.METHOD_DECLARATION,
            E.CONSTRUCTOR_DECLARATION,
            E.OPERATOR_DECLARATION,
            E.FIELD_DECLARATION,
            E.CONST_DECLARATION,
            E.PROPERTY_DECLARATION,
            E.INIT_BLOCK,
            E.STATIC_BLOCK,
            E.DROP_BLOCK,
            E.CLASS_DECLARATION,
            E.INTERFACE_DECLARATION,
            E.ENUM_DECLARATION,
            E.RECORD_DECLARATION,
            E.STRUCT_DECLARATION,
            E.ANNOTATION_DECLARATION,
            E.TYPE_ALIAS_DECLARATION,
        )
    }
}
