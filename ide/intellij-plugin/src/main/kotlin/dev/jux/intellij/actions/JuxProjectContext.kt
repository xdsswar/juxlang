package dev.jux.intellij.actions

import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.LangDataKeys
import com.intellij.openapi.vfs.VirtualFile

/**
 * "Is this action being invoked inside a Jux project?"
 *
 * A Jux module is a directory holding a `jux.toml`, so the question is
 * answered by walking outward from whatever the action was invoked on until
 * one turns up or the project's own root is passed. The same rule the icon
 * provider and the semantic annotator use, kept in one place because three
 * different answers to it would be three different behaviours.
 */
internal object JuxProjectContext {

    /** The directory an action is about, from whichever data key carries it. */
    fun targetDirectory(e: AnActionEvent): VirtualFile? {
        e.getData(LangDataKeys.IDE_VIEW)?.directories?.firstOrNull()?.let { return it.virtualFile }
        val file = e.getData(CommonDataKeys.VIRTUAL_FILE) ?: return null
        return if (file.isDirectory) file else file.parent
    }

    /**
     * True when [start], or an ancestor at or inside the project root, holds a
     * `jux.toml`.
     *
     * Stops at the project root rather than walking to the filesystem root: a
     * `jux.toml` somewhere above an unrelated project is not this project's.
     */
    fun inJuxModule(e: AnActionEvent): Boolean {
        val projectRoot = e.project?.basePath
        var dir: VirtualFile? = targetDirectory(e) ?: return false
        var hops = 0
        while (dir != null && hops < 64) {
            if (dir.findChild("jux.toml") != null) return true
            if (projectRoot != null && dir.path == projectRoot) return false
            dir = dir.parent
            hops++
        }
        return false
    }
}
