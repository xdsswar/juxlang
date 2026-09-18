package dev.jux.intellij.editor

import com.intellij.codeInsight.editorActions.JoinLinesHandlerDelegate.CANNOT_JOIN
import com.intellij.codeInsight.editorActions.JoinRawLinesHandlerDelegate
import com.intellij.openapi.editor.Document
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.intentions.JuxCodeShapes as S
import dev.jux.intellij.intentions.JuxJoinDeclarationIntention
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile

/**
 * Join Lines (Ctrl+Shift+J) for Jux, the smart joins Java's editor makes
 * instead of just pulling the next line up:
 *
 * - `int x;` then `x = 5;` becomes `int x = 5;` (Java's
 *   `DeclarationJoinLinesHandler`);
 * - `"Hello, " +` then `"world"` becomes `"Hello, world"` (the adjacent
 *   literals of a concatenation fuse);
 * - `if (a)` then `if (b) ...`, neither with an `else`, becomes
 *   `if (a && b) ...` (with or without braces around the inner `if`).
 *
 * It is a raw joiner: it sees the document before the line break is removed,
 * with [start] just after the first line's last character and [end] on the
 * second line's first one.
 */
class JuxJoinLinesHandler : JoinRawLinesHandlerDelegate {

    override fun tryJoinLines(document: Document, file: PsiFile, start: Int, end: Int): Int = CANNOT_JOIN

    override fun tryJoinRawLines(document: Document, file: PsiFile, start: Int, end: Int): Int {
        if (file !is JuxFile || start <= 0) return CANNOT_JOIN
        val last = file.findElementAt(start - 1) ?: return CANNOT_JOIN
        val next = file.findElementAt(end) ?: return CANNOT_JOIN
        return joinDeclaration(document, last)
            ?: joinStrings(document, last, next)
            ?: joinIfs(document, last, next)
            ?: CANNOT_JOIN
    }

    /** `int x;` + `x = 5;`: the declaration takes the value. */
    private fun joinDeclaration(document: Document, last: PsiElement): Int? {
        if (last.elementType !== T.SEMICOLON) return null
        val local = last.parent?.takeIf { it.elementType === E.LOCAL_VARIABLE } ?: return null
        val statement = JuxJoinDeclarationIntention.assignmentFor(local) ?: return null
        val value = S.right(S.compositeChildren(statement).first()) ?: return null
        val decl = local.text.trimEnd().removeSuffix(";").trimEnd()
        val text = "$decl = ${value.text};"
        document.replaceString(local.textRange.startOffset, statement.textRange.endOffset, text)
        return local.textRange.startOffset + decl.length + 1
    }

    /** `"a" +` + `"b"`: two adjacent plain literals fuse into one. */
    private fun joinStrings(document: Document, last: PsiElement, next: PsiElement): Int? {
        if (last.elementType !== T.PLUS || next.elementType !== T.STRING_LITERAL) return null
        val binary = last.parent?.takeIf { it.elementType === E.BINARY_EXPRESSION } ?: return null
        val right = S.right(binary)?.takeIf { it.firstChild == next } ?: return null
        val leftLiteral = rightmostLeaf(S.left(binary) ?: return null)
        if (leftLiteral?.elementType !== T.STRING_LITERAL) return null
        if (isRaw(leftLiteral.text) || isRaw(next.text)) return null
        val joinAt = leftLiteral.textRange.endOffset - 1
        val merged = leftLiteral.text.dropLast(1) + next.text.drop(1)
        document.replaceString(leftLiteral.textRange.startOffset, right.textRange.endOffset, merged)
        return joinAt
    }

    /** `if (a)` / `if (a) {` + `if (b) ...`: one `if` over `a && b`. */
    private fun joinIfs(document: Document, last: PsiElement, next: PsiElement): Int? {
        if (next.elementType !== T.IF_KW) return null
        val inner = next.parent?.takeIf { it.elementType === E.IF_STATEMENT } ?: return null
        val outer = when (last.elementType) {
            T.RPAREN -> last.parent
            T.LBRACE -> last.parent?.parent
            else -> null
        }?.takeIf { it.elementType === E.IF_STATEMENT } ?: return null
        if (S.ifElse(outer) != null || S.ifElse(inner) != null) return null
        val body = S.ifThen(outer) ?: return null
        if (body != inner && (body.elementType !== E.CODE_BLOCK || S.singleStatement(body) != inner)) return null
        if (last.elementType == T.RPAREN && body != inner) return null
        val and = S.precedence(T.AND_AND)
        val a = S.operandText(S.ifCondition(outer) ?: return null, and)
        val b = S.operandText(S.ifCondition(inner) ?: return null, and)
        val then = S.ifThen(inner)?.text ?: return null
        val head = "if ($a && $b) "
        val start = outer.textRange.startOffset
        document.replaceString(start, outer.textRange.endOffset, head + then)
        // The inner body moved out one level: reindent it.
        val caret = document.createRangeMarker(start + head.length - 2, start + head.length - 2)
        val written = document.createRangeMarker(start, start + head.length + then.length)
        val project = last.project
        com.intellij.psi.PsiDocumentManager.getInstance(project).commitDocument(document)
        val file = com.intellij.psi.PsiDocumentManager.getInstance(project).getPsiFile(document) ?: return caret.startOffset
        com.intellij.psi.codeStyle.CodeStyleManager.getInstance(project)
            .reformatText(file, written.startOffset, written.endOffset)
        return caret.startOffset
    }

    private fun rightmostLeaf(e: PsiElement): PsiElement? {
        var cur: PsiElement? = e.lastChild
        while (cur != null && (cur is PsiWhiteSpace || cur.firstChild != null)) {
            cur = if (cur is PsiWhiteSpace) cur.prevSibling else cur.lastChild
        }
        return cur
    }

    private fun isRaw(text: String) = text.startsWith("\"\"\"")
}
