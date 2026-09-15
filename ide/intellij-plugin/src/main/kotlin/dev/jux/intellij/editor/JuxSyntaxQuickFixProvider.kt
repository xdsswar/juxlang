package dev.jux.intellij.editor

import com.intellij.codeInsight.daemon.impl.HighlightInfo
import com.intellij.codeInsight.daemon.impl.analysis.ErrorQuickFixProvider
import com.intellij.codeInsight.intention.IntentionAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiErrorElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.JuxLanguage

/**
 * Alt+Enter on a syntax error: the fix the message already names.
 *
 * "';' expected" inserts the `;` right after the last token (not where the
 * error was reported, which may be the start of the next line), a stray `)`
 * is removed, and a Java-style `case 1:` has its `:` replaced with `->`.
 */
class JuxSyntaxQuickFixProvider : ErrorQuickFixProvider {

    override fun registerErrorQuickFix(errorElement: PsiErrorElement, builder: HighlightInfo.Builder) {
        if (errorElement.language !== JuxLanguage) return
        val message = errorElement.errorDescription
        val fix: IntentionAction? = when {
            message in INSERTABLE -> {
                val token = INSERTABLE.getValue(message)
                InsertTokenFix(afterPreviousToken(errorElement), token)
            }
            message == "'->' expected" && errorElement.text == ":" -> {
                // `case 1:` becomes `case 1 ->`, spaced the way the arm is written.
                val spaced = PsiTreeUtil.prevLeaf(errorElement) !is PsiWhiteSpace
                ReplaceTextFix(
                    errorElement.textRange.startOffset,
                    errorElement.textRange.endOffset,
                    if (spaced) " ->" else "->",
                    "Replace ':' with '->'",
                )
            }
            message.startsWith("Unexpected '") && errorElement.textLength > 0 ->
                ReplaceTextFix(
                    errorElement.textRange.startOffset,
                    errorElement.textRange.endOffset,
                    "",
                    "Remove ${message.removePrefix("Unexpected ")}",
                )
            else -> null
        }
        if (fix != null) builder.registerFix(fix, null, null, null, null)
    }

    /** The offset just after the last real token before [element]. */
    private fun afterPreviousToken(element: PsiErrorElement): Int {
        var prev = PsiTreeUtil.prevLeaf(element)
        while (prev != null && (prev is PsiWhiteSpace || prev is PsiComment || prev.textLength == 0)) {
            prev = PsiTreeUtil.prevLeaf(prev)
        }
        return prev?.textRange?.endOffset ?: element.textRange.startOffset
    }

    private class InsertTokenFix(private val offset: Int, private val token: String) : IntentionAction {
        override fun getText(): String = "Insert '$token'"
        override fun getFamilyName(): String = "Insert missing token"
        override fun isAvailable(project: Project, editor: Editor?, file: PsiFile?): Boolean =
            editor != null && offset <= editor.document.textLength
        override fun invoke(project: Project, editor: Editor?, file: PsiFile?) {
            val document = editor?.document ?: return
            // A missing `}` goes on its own line, the others where the token ends.
            val text = if (token == "}") "\n$token" else token
            document.insertString(offset, text)
            editor.caretModel.moveToOffset(offset + text.length)
        }
        override fun startInWriteAction(): Boolean = true
    }

    private class ReplaceTextFix(
        private val start: Int,
        private val end: Int,
        private val replacement: String,
        private val label: String,
    ) : IntentionAction {
        override fun getText(): String = label
        override fun getFamilyName(): String = "Fix syntax"
        override fun isAvailable(project: Project, editor: Editor?, file: PsiFile?): Boolean =
            editor != null && end <= editor.document.textLength
        override fun invoke(project: Project, editor: Editor?, file: PsiFile?) {
            editor?.document?.replaceString(start, end, replacement)
        }
        override fun startInWriteAction(): Boolean = true
    }

    private companion object {
        val INSERTABLE = mapOf(
            "';' expected" to ";",
            "')' expected" to ")",
            "']' expected" to "]",
            "'}' expected" to "}",
            "'>' expected" to ">",
            "',' or ')' expected" to ",",
        )
    }
}
