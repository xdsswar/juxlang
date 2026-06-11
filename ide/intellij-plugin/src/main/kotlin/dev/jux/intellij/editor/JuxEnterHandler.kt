package dev.jux.intellij.editor

import com.intellij.codeInsight.editorActions.enter.EnterHandlerDelegate
import com.intellij.codeInsight.editorActions.enter.EnterHandlerDelegateAdapter
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.actionSystem.EditorActionHandler
import com.intellij.openapi.util.Ref
import com.intellij.psi.PsiFile
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.highlight.JuxTokenTypes

/**
 * Java-like Enter behaviour inside comments:
 *
 * - `/**` + Enter expands to a doc-comment skeleton with the caret on an
 *   aligned ` * ` line and `*/` closed below.
 * - Enter on a `*`-continuation line starts the next line with an aligned `* `.
 *
 * Everything is document-text based (no PSI) and fully guarded, so it can never
 * break the normal Enter key.
 */
class JuxEnterHandler : EnterHandlerDelegateAdapter() {
    override fun preprocessEnter(
        file: PsiFile,
        editor: Editor,
        caretOffset: Ref<Int>,
        caretAdvance: Ref<Int>,
        dataContext: DataContext,
        originalHandler: EditorActionHandler?,
    ): EnterHandlerDelegate.Result {
        try {
            if (file.fileType != JuxFileType) return EnterHandlerDelegate.Result.Continue

            val document = editor.document
            val offset = caretOffset.get()
            if (offset < 3 || offset > document.textLength) return EnterHandlerDelegate.Result.Continue

            val chars = document.charsSequence
            val lineNumber = document.getLineNumber(offset)
            val lineStart = document.getLineStartOffset(lineNumber)
            val prefix = chars.subSequence(lineStart, offset).toString()
            val trimmed = prefix.trim()
            val indent = prefix.takeWhile { it == ' ' || it == '\t' }

            // The comment-context guards below read PSI; sync it with the
            // just-typed text first (cheap, and preprocessEnter runs inside
            // the command's write action).
            if (trimmed == "/**" || trimmed.startsWith("*")) {
                com.intellij.psi.PsiDocumentManager.getInstance(file.project)
                    .commitDocument(document)
            }

            // Case 1: caret immediately after "/**" → expand the doc skeleton.
            // PSI guard: the `/**` must actually BE a comment token — the same
            // characters inside a raw string (`"""/**"""`) must not expand.
            if (trimmed == "/**" && inComment(file, offset)) {
                // Bounded look-ahead (never scan the whole file on the UI thread).
                if (!isClosedAhead(chars, offset, document.textLength)) {
                    val insertion = "\n$indent * \n$indent */"
                    document.insertString(offset, insertion)
                    val caret = offset + 1 + indent.length + 3 // "\n" + indent + " * "
                    caretOffset.set(caret)
                    caretAdvance.set(0)
                    editor.caretModel.moveToOffset(caret)
                    return EnterHandlerDelegate.Result.Stop
                }
            }

            // Case 2: continuing a "* ..." line — ONLY inside a block/doc
            // comment. A line that merely *starts* with `*` (a wrapped
            // multiplication operand, a markdown bullet inside a raw string)
            // must keep the plain Enter.
            if (trimmed.startsWith("*") && !trimmed.startsWith("*/") && inComment(file, offset)) {
                val insertion = "\n$indent* "
                document.insertString(offset, insertion)
                val caret = offset + insertion.length
                caretOffset.set(caret)
                caretAdvance.set(0)
                editor.caretModel.moveToOffset(caret)
                return EnterHandlerDelegate.Result.Stop
            }
        } catch (_: Exception) {
            // Never break the Enter key.
        }
        return EnterHandlerDelegate.Result.Continue
    }

    /**
     * Is the next non-whitespace run after `from` the comment-closing sequence
     * (star then slash)? Scans at most a small window so this stays O(1) on the
     * UI thread regardless of file size.
     */
    private fun isClosedAhead(chars: CharSequence, from: Int, end: Int): Boolean {
        val limit = minOf(end, from + 64)
        var i = from
        while (i < limit && chars[i].isWhitespace()) i++
        return i + 1 < end && chars[i] == '*' && chars[i + 1] == '/'
    }

    /**
     * True when the caret sits inside (or right at the end of) a block / doc /
     * line comment token. Checks the leaf at the offset and, for a caret
     * parked just past a token, the leaf before it — covering both "typing
     * inside the comment" and "Enter at end-of-line".
     */
    private fun inComment(file: PsiFile, offset: Int): Boolean {
        val at = file.findElementAt(offset)?.node?.elementType
        if (at === JuxTokenTypes.BLOCK_COMMENT || at === JuxTokenTypes.DOC_COMMENT ||
            at === JuxTokenTypes.LINE_COMMENT
        ) {
            return true
        }
        if (offset > 0) {
            val before = file.findElementAt(offset - 1)?.node?.elementType
            return before === JuxTokenTypes.BLOCK_COMMENT || before === JuxTokenTypes.DOC_COMMENT ||
                before === JuxTokenTypes.LINE_COMMENT
        }
        return false
    }
}
