package dev.jux.intellij.project

import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.module.ModuleManager
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import com.intellij.openapi.roots.ModuleRootManager
import com.intellij.psi.search.FilenameIndex
import com.intellij.psi.search.GlobalSearchScope
import dev.jux.intellij.JuxPackageResolver

/**
 * The `jux.sourceRoots` configuration the plugin gives `juxc-lsp` (§I.4
 * "Coordination with `juxc-lsp`"): every Jux package root in the project, so
 * the server's cross-file features work over the same files and packages the
 * IDE does.
 *
 * Each entry is `{ "path": <absolute path>, "kind": "sources" | "tests",
 * "origin": "marked" | "manifest" }`: a directory marked as a Jux root, and,
 * for a `jux.toml` project with nothing marked, the `src/` and `test/` the
 * resolver reads from the manifest. A root appears once.
 *
 * Plain maps and lists, so both LSP clients can serialize it without this
 * class touching either client's API: the native client answers
 * `workspace/configuration` with it (`dev.jux.intellij.lsp`), LSP4IJ with its
 * JSON form (`dev.jux.intellij.lsp4ij`).
 */
object JuxSourceRootsConfig {

    /** The configuration section a server asks for. */
    const val SECTION = "jux.sourceRoots"

    /** The roots, as described above. Safe to call from any thread. */
    fun compute(project: Project): List<Map<String, String>> =
        ReadAction.compute<List<Map<String, String>>, RuntimeException> {
            if (project.isDisposed) emptyList() else computeInReadAction(project)
        }

    private fun computeInReadAction(project: Project): List<Map<String, String>> {
        val out = LinkedHashMap<String, Map<String, String>>()
        for (module in ModuleManager.getInstance(project).modules) {
            for (entry in ModuleRootManager.getInstance(module).contentEntries) {
                for (folder in entry.sourceFolders) {
                    val type = folder.rootType as? JuxSourceRootType ?: continue
                    val dir = folder.file ?: continue
                    out.putIfAbsent(dir.path, entry(dir.path, type.isForTests, "marked"))
                }
            }
        }
        // Manifest roots need the file index; during indexing they are left
        // out, and the next change notification brings them.
        if (!DumbService.isDumb(project)) {
            val manifests = FilenameIndex.getVirtualFilesByName(JuxPackageResolver.MANIFEST, GlobalSearchScope.projectScope(project))
            for (manifest in manifests.sortedBy { it.path }) {
                val dir = manifest.parent ?: continue
                val src = dir.findChild("src")?.takeIf { it.isDirectory }
                val test = dir.findChild("test")?.takeIf { it.isDirectory }
                out.putIfAbsent((src ?: dir).path, entry((src ?: dir).path, false, "manifest"))
                if (test != null) out.putIfAbsent(test.path, entry(test.path, true, "manifest"))
            }
        }
        return out.values.toList()
    }

    private fun entry(path: String, tests: Boolean, origin: String): Map<String, String> =
        mapOf("path" to path, "kind" to if (tests) "tests" else "sources", "origin" to origin)

    /** The value to answer [section] with, or `null` when it is not ours. */
    fun answer(project: Project, section: String?): Any? = when (section) {
        SECTION -> compute(project)
        "jux" -> mapOf("sourceRoots" to compute(project))
        else -> null
    }
}
