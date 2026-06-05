package dev.jux.intellij.editor

import com.intellij.codeInsight.editorActions.enter.EnterHandlerDelegate
import com.intellij.codeInsight.editorActions.enter.EnterHandlerDelegateAdapter
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.editor.actionSystem.EditorActionHandler
import com.intellij.openapi.util.Ref
import com.intellij.psi.PsiFile
import dev.jux.intellij.JuxFileType

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

            // Case 1: caret immediately after "/**" → expand the doc skeleton.
            if (trimmed == "/**") {
                val rest = chars.subSequence(offset, document.textLength).toString()
                if (!rest.trimStart().startsWith("*/")) {
                    val insertion = "\n$indent * \n$indent */"
                    document.insertString(offset, insertion)
                    val caret = offset + 1 + indent.length + 3 // "\n" + indent + " * "
                    caretOffset.set(caret)
                    caretAdvance.set(0)
                    editor.caretModel.moveToOffset(caret)
                    return EnterHandlerDelegate.Result.Stop
                }
            }

            // Case 2: continuing a "* ..." line inside a block/doc comment.
            if (trimmed.startsWith("*") && !trimmed.startsWith("*/")) {
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
}
