package dev.jux.intellij.intentions

import com.intellij.codeInsight.hint.HintManager
import com.intellij.codeInsight.hint.QuestionAction
import com.intellij.codeInsight.intention.HighPriorityAction
import com.intellij.codeInspection.HintAction
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.popup.JBPopupFactory
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.SmartPointerManager
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.psi.JuxFile

/**
 * "Import 'some.Truck'" on a type name that needs an import: Alt+Enter, the
 * question hint that pops up by itself over the name (Java's
 * "some.Truck? Alt+Enter"), and the chooser when two packages declare the same
 * name.
 */
class JuxImportTypeFix(
    use: PsiElement,
    private val name: String,
    private val fqns: List<String>,
) : HintAction, HighPriorityAction {

    private val pointer = SmartPointerManager.createPointer(use)

    override fun getText(): String = fqns.singleOrNull()?.let { "Import '$it'" } ?: "Import type…"

    override fun getFamilyName(): String = "Import type"

    override fun startInWriteAction(): Boolean = false

    override fun isAvailable(project: Project, editor: Editor?, file: PsiFile?): Boolean =
        file is JuxFile && pointer.element?.isValid == true && fqns.isNotEmpty()

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?) {
        val juxFile = file as? JuxFile ?: return
        if (fqns.size == 1 || editor == null) {
            addImport(project, juxFile, fqns.first())
            return
        }
        JBPopupFactory.getInstance()
            .createPopupChooserBuilder(fqns)
            .setTitle("Import Type")
            .setItemChosenCallback { addImport(project, juxFile, it) }
            .createPopup()
            .showInBestPositionFor(editor)
    }

    /**
     * The auto-import question hint over the name. Shown only for a single
     * candidate the caret is not typing into, the way Java's is.
     */
    override fun showHint(editor: Editor): Boolean {
        val use = pointer.element ?: return false
        val file = use.containingFile as? JuxFile ?: return false
        if (fqns.isEmpty()) return false
        val range = use.textRange
        if (editor.caretModel.offset in range.startOffset..range.endOffset) return false
        if (HintManager.getInstance().hasShownHintsThatWillHideByOtherHint(true)) return false
        val message = (fqns.singleOrNull() ?: "${fqns.first()} (+${fqns.size - 1})") + "? " +
            com.intellij.openapi.keymap.KeymapUtil.getFirstKeyboardShortcutText("ShowIntentionActions")
        HintManager.getInstance().showQuestionHint(
            editor,
            message,
            range.startOffset,
            range.endOffset,
            QuestionAction {
                invoke(use.project, editor, file)
                true
            },
        )
        return true
    }

    private fun addImport(project: Project, file: JuxFile, fqn: String) {
        val document = file.viewProvider.document ?: return
        WriteCommandAction.runWriteCommandAction(project, "Import Type", null, {
            JuxAutoImport.addImport(project, document, file, fqn, name)
        }, file)
    }
}
