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
 * **New → Jux Module** — a directory with a `jux.toml` and a `src/`, which is
 * what makes it a module the toolchain will build.
 *
 * A multi-package project is several of these, and the alternative was
 * creating the directory, then the manifest, then remembering the two keys it
 * needs. The name doubles as the package name, dotted like every other Jux
 * package (`demo.ui`), so the directory is the last segment.
 */
class NewJuxModuleAction :
    AnAction("Jux Module", "Create a Jux module (a directory with jux.toml)", AllIcons.Nodes.Module) {

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val view = e.getData(LangDataKeys.IDE_VIEW) ?: return
        val parent: PsiDirectory = view.orChooseDirectory ?: return

        val name = Messages.showInputDialog(
            project,
            "Module name, dotted like a package (e.g. demo.ui):",
            "New Jux Module",
            null,
        )?.trim().orEmpty()
        if (name.isEmpty()) return

        // The directory takes the last segment; the manifest keeps the whole
        // dotted name, which is what the toolchain resolves dependencies by.
        val dirName = name.substringAfterLast('.')
        if (dirName.isEmpty()) return

        WriteCommandAction.runWriteCommandAction(project) {
            val moduleDir = parent.findSubdirectory(dirName) ?: parent.createSubdirectory(dirName)
            if (moduleDir.findFile("jux.toml") == null) {
                val manifest = moduleDir.createFile("jux.toml")
                manifest.virtualFile.setBinaryContent(
                    MANIFEST.format(name).toByteArray(Charsets.UTF_8)
                )
            }
            val src = moduleDir.findSubdirectory("src") ?: moduleDir.createSubdirectory("src")
            view.selectElement(src)
        }
    }

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabledAndVisible =
            e.project != null && e.getData(LangDataKeys.IDE_VIEW) != null
    }

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    private companion object {
        val MANIFEST = """
            [package]
            name = "%s"
            version = "0.1.0"
        """.trimIndent() + "\n"
    }
}
