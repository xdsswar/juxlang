package dev.jux.intellij.codeInsight

import com.intellij.codeInsight.hint.HintManager
import com.intellij.lang.LanguageCodeInsightActionHandler
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile

/**
 * **Implement Methods** (Ctrl+I) for Jux — the Java-plugin experience: from
 * the class at the caret, list the inherited **abstract** methods (interface
 * methods without a `default` body, `abstract` class methods) the class has
 * not declared, and insert `@override` stubs for the chosen ones.
 */
class JuxImplementMembersHandler : LanguageCodeInsightActionHandler {

    override fun isValidFor(editor: Editor?, file: PsiFile?): Boolean {
        if (editor == null || file == null) return false
        return JuxOverrideMembers.typeAtCaret(editor, file) != null
    }

    override fun invoke(project: Project, editor: Editor, file: PsiFile) {
        val type = JuxOverrideMembers.typeAtCaret(editor, file) ?: return
        val handled = JuxOverrideMembers.chooseAndInsert(
            project, editor, type,
            setOf(JuxOverrideMembers.Kind.IMPLEMENT),
            "Select Methods to Implement",
        )
        if (!handled) {
            HintManager.getInstance().showInformationHint(editor, "No methods to implement")
        }
    }

    override fun startInWriteAction(): Boolean = false // the engine opens its own write command
}
