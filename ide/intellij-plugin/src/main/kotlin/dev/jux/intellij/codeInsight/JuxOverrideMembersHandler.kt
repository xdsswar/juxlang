package dev.jux.intellij.codeInsight

import com.intellij.codeInsight.hint.HintManager
import com.intellij.lang.LanguageCodeInsightActionHandler
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile

/**
 * **Override Methods** (Ctrl+O) for Jux: from the class at the caret, list the
 * inherited **concrete** methods (those with bodies) it could override, and
 * insert `@override` stubs that delegate to `super.method(...)`.
 */
class JuxOverrideMembersHandler : LanguageCodeInsightActionHandler {

    override fun isValidFor(editor: Editor?, file: PsiFile?): Boolean {
        if (editor == null || file == null) return false
        return JuxOverrideMembers.typeAtCaret(editor, file) != null
    }

    override fun invoke(project: Project, editor: Editor, file: PsiFile) {
        val type = JuxOverrideMembers.typeAtCaret(editor, file) ?: return
        val handled = JuxOverrideMembers.chooseAndInsert(
            project, editor, type,
            setOf(JuxOverrideMembers.Kind.OVERRIDE),
            "Select Methods to Override",
        )
        if (!handled) {
            HintManager.getInstance().showInformationHint(editor, "No methods to override")
        }
    }

    override fun startInWriteAction(): Boolean = false // the engine opens its own write command
}
