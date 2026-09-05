package dev.jux.intellij.resolve

import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.VirtualFileManager
import dev.jux.intellij.run.JuxToolchain
import java.io.File

/**
 * Where the standard library and the bound Rust crates actually live on THIS
 * machine, discovered from the toolchain rather than described in the plugin.
 *
 * The compiler writes every foreign API it binds as a `.jux.d` stub — Jux
 * source declaring the crate's real types and members, generated from that
 * crate's own rustdoc. `rust.std` lands in the user cache; each project's other
 * crates land in its `.jux-stubs/`. Those files are the standard library and
 * the crate surface, exactly as the compiler sees them.
 *
 * Because they are Jux source, indexing them costs nothing beyond pointing the
 * IDE at the directory: the existing parser, PSI, type index, completion and
 * go-to-definition then work on `Vec`, `HashMap`, `minifb::Window` and anything
 * else the project binds, with the members the installed toolchain actually
 * has. Nothing here is a list of names — a plugin that shipped one would be
 * wrong the first time the standard library changed, and would stay wrong for
 * whichever crates the user happens to depend on.
 *
 * Discovery order mirrors the compiler's own (`juxc-driver`'s `stubs.rs`):
 *
 *  1. `$JUX_STUBS_DIR` — the explicit override.
 *  2. The OS user-cache root: `%LOCALAPPDATA%` on Windows, `$XDG_CACHE_HOME`,
 *     `~/.cache`, or `~/Library/Caches` on macOS, each under `juxc/stubs`.
 *  3. The toolchain install root's own `stubs/`, for a portable install whose
 *     cache the user cannot write.
 *
 * Roots that do not exist are dropped, so an unconfigured machine simply gets
 * no extra index rather than an error.
 */
object JuxStubRoots {

    /** Every existing stub directory outside the project, in discovery order. */
    fun externalRoots(): List<File> {
        val out = LinkedHashSet<File>()

        env("JUX_STUBS_DIR")?.let { out.add(File(it)) }

        userCacheDirs().forEach { out.add(File(File(it, "juxc"), "stubs")) }

        // A portable toolchain ships its stubs beside the executables.
        JuxToolchain.find("juxc")?.let { exe ->
            val bin = File(exe).parentFile ?: return@let
            out.add(File(bin, "stubs"))
            bin.parentFile?.let { out.add(File(it, "stubs")) }
        }

        return out.filter { it.isDirectory }
    }

    /** [externalRoots] as VFS directories, refreshed so a freshly generated
     *  stub is visible without restarting the IDE. */
    fun externalRootFiles(): List<VirtualFile> {
        val lfs = LocalFileSystem.getInstance()
        return externalRoots().mapNotNull { lfs.refreshAndFindFileByIoFile(it) }
    }

    /**
     * The project's own `.jux-stubs/` directories — one per package that binds
     * a crate. These are inside the content root, so they are already indexed;
     * this is for callers that want to name them (the tool window, a "rebuild
     * stubs" action, the staleness check).
     */
    fun projectStubDirs(project: Project): List<VirtualFile> {
        val base = project.basePath ?: return emptyList()
        val root = VirtualFileManager.getInstance().findFileByNioPath(File(base).toPath())
            ?: return emptyList()
        val out = ArrayList<VirtualFile>()
        collectStubDirs(root, out, depth = 0)
        return out
    }

    private fun collectStubDirs(dir: VirtualFile, out: MutableList<VirtualFile>, depth: Int) {
        if (depth > 6) return
        for (child in dir.children) {
            if (!child.isDirectory) continue
            when (child.name) {
                STUB_DIRNAME -> out.add(child)
                // Build output holds copies, not sources; skip it and the VCS dir.
                "target", ".git", "node_modules" -> {}
                else -> collectStubDirs(child, out, depth + 1)
            }
        }
    }

    private fun userCacheDirs(): List<File> {
        val out = ArrayList<File>()
        env("LOCALAPPDATA")?.let { out.add(File(it)) }
        env("XDG_CACHE_HOME")?.let { out.add(File(it)) }
        val home = env("HOME") ?: env("USERPROFILE")
        if (home != null) {
            out.add(File(File(home, ".cache").path))
            out.add(File(File(File(home, "Library"), "Caches").path))
        }
        return out
    }

    private fun env(name: String): String? = try {
        System.getenv(name)?.takeIf { it.isNotBlank() }
    } catch (_: SecurityException) {
        null
    }

    /** The directory name the compiler uses for a package's generated stubs. */
    const val STUB_DIRNAME = ".jux-stubs"
}
