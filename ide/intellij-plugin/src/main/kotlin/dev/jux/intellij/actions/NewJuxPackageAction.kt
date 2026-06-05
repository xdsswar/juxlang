package dev.jux.intellij.actions

import com.intellij.icons.AllIcons
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.LangDataKeys
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.ui.Messages
import com.intellij.psi.PsiDirectory

/**
 * **New → Jux Package** — creates a nested directory chain from a dotted name
 * (`com.example.foo` → `com/example/foo/`), Java-style, under the selected
 * directory. The compiler derives package identity from the directory path, so
 * this is how you lay out packages.
 */
class NewJuxPackageAction :
    AnAction("Jux Package", "Create a new Jux package (nested directories)", AllIcons.Nodes.Package) {

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val view = e.getData(LangDataKeys.IDE_VIEW) ?: return
        val dir: PsiDirectory = view.orChooseDirectory ?: return

        val input = Messages.showInputDialog(
            project,
            "Enter package name (e.g. com.example.foo):",
            "New Jux Package",
            null,
        )?.trim().orEmpty()
        if (input.isEmpty()) return

        val segments = input.split('.').map { it.trim() }.filter { it.isNotEmpty() }
        if (segments.isEmpty()) return

        WriteCommandAction.runWriteCommandAction(project) {
            var current = dir
            for (seg in segments) {
                current = current.findSubdirectory(seg) ?: current.createSubdirectory(seg)
            }
            view.selectElement(current)
        }
    }

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabledAndVisible =
            e.project != null && e.getData(LangDataKeys.IDE_VIEW) != null
    }

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT
}
