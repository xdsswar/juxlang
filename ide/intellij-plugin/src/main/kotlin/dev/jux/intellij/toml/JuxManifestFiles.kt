package dev.jux.intellij.toml

import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.vfs.VfsUtilCore
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiFile

/**
 * Where a `jux.toml` sits in its workspace, read through the IDE's virtual
 * files (so unsaved edits in an open root manifest count). Shared by the
 * manifest inspection, completion and go-to-declaration.
 */
object JuxManifestFiles {

    const val MANIFEST = "jux.toml"

    /** True when [file] is a Jux manifest. */
    fun isManifest(file: PsiFile?): Boolean = file?.name == MANIFEST

    /**
     * The workspace root manifest [manifest] belongs to: the nearest
     * `jux.toml` at or above its directory whose text has a `[workspace]`
     * table. A root is its own workspace root.
     */
    fun workspaceRoot(manifest: VirtualFile): VirtualFile? {
        var dir: VirtualFile? = manifest.parent
        while (dir != null) {
            val candidate = dir.findChild(MANIFEST)
            if (candidate != null && JuxTomlModel.isWorkspaceRoot(textOf(candidate))) return candidate
            dir = dir.parent
        }
        return null
    }

    /** A file's text: the open document's if it is being edited, else the disk content. */
    fun textOf(file: VirtualFile): String = try {
        FileDocumentManager.getInstance().getCachedDocument(file)?.text ?: VfsUtilCore.loadText(file)
    } catch (_: Exception) {
        ""
    }

    /** A [JuxTomlModel.DirView] over a virtual directory. */
    class VirtualDirView(private val root: VirtualFile) : JuxTomlModel.DirView {
        private fun at(rel: String): VirtualFile? = if (rel.isEmpty()) root else root.findFileByRelativePath(rel)
        override fun childDirs(rel: String): List<String> =
            at(rel)?.children?.filter { it.isDirectory }?.map { it.name }.orEmpty()
        override fun isDirectory(rel: String): Boolean = at(rel)?.isDirectory == true
        override fun hasManifest(rel: String): Boolean = at(rel)?.findChild(MANIFEST)?.isDirectory == false
    }
}
