package dev.jux.intellij.refactoring

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import dev.jux.intellij.psi.JuxFile

/**
 * Refactor > Extract/Introduce > Parameter Object, for Jux.
 *
 * The platform's own item finds its implementation through an extension that
 * is built around Java's class model, so for Jux it would stay greyed out. This
 * action, shown only in a Jux file, runs [JuxIntroduceParameterObjectHandler].
 */
class JuxIntroduceParameterObjectAction : AnAction() {
    override fun getActionUpdateThread() = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabledAndVisible = e.getData(CommonDataKeys.PSI_FILE) is JuxFile &&
            e.getData(CommonDataKeys.EDITOR) != null
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        JuxIntroduceParameterObjectHandler().invoke(
            project, e.getData(CommonDataKeys.EDITOR), e.getData(CommonDataKeys.PSI_FILE), e.dataContext,
        )
    }
}
