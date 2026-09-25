package dev.jux.intellij

import com.intellij.openapi.module.ModuleUtilCore
import com.intellij.openapi.project.Project
import com.intellij.openapi.roots.ModuleRootManager
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.roots.ProjectFileIndex
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VfsUtilCore
import com.intellij.openapi.vfs.VirtualFile
import dev.jux.intellij.project.JuxSourceRootType

/**
 * Derives a Jux `package` from a file's location, the way the Java plugin
 * treats `.java` files (§I.4): the package is the directory's path below its
 * package root, with `/` read as `.`. A file directly in the root has no
 * package, and the templates omit the `package` line for it.
 *
 * This is the ONE implementation. The New File templates, the
 * package-mismatch inspection, Move Class and the Project view's flattened
 * packages all ask it, so they can never disagree about where a package
 * lives.
 *
 * **Which root.** The first rule that applies wins:
 *
 *  1. The nearest directory marked **Jux Sources Root** or **Jux Test Sources
 *     Root** (see [JuxSourceRootType]).
 *  2. The nearest `jux.toml` (§B.1): below it, `src/` is the sources root and
 *     `test/` the tests root; a manifest with no `src/` makes its own
 *     directory the root. A file under a manifest but outside those (its
 *     `examples/`, `docs/`, `target/`) is in no root at all, because each of
 *     those files stands alone and takes its package from its own
 *     declaration (§B.1.3).
 *  3. Any other source root the IDE knows about, then the content root. This
 *     last rule is only a convenience for the New File templates in a
 *     project that marked nothing: it is not a claim about the Jux layout,
 *     so [Root.authoritative] is false and the mismatch inspection ignores
 *     it.
 */
object JuxPackageResolver {

    /** The manifest file that marks a Jux project directory (§B.2). */
    const val MANIFEST = "jux.toml"

    /** How a package root was found, and so how much it can be trusted. */
    enum class Kind(val authoritative: Boolean, val tests: Boolean) {
        /** A directory marked Jux Sources Root. */
        MARKED_SOURCES(true, false),

        /** A directory marked Jux Test Sources Root. */
        MARKED_TESTS(true, true),

        /** `src/` below a `jux.toml`, or the manifest directory itself when it has no `src/`. */
        MANIFEST_SOURCES(true, false),

        /** `test/` below a `jux.toml`. */
        MANIFEST_TESTS(true, true),

        /** A non-Jux source root or a content root: a guess, good enough for a template. */
        FALLBACK(false, false),
    }

    /** A package root: the directory whose relative paths are package names. */
    data class Root(val dir: VirtualFile, val kind: Kind) {
        /** True when the Jux layout itself says this is the root (rules 1 and 2). */
        val authoritative: Boolean get() = kind.authoritative

        /** True for a test root, whose files may add `.test` to the package (§B.1.2). */
        val tests: Boolean get() = kind.tests
    }

    /** Infer the package for a file or directory, or `null` when no root contains it. */
    fun inferPackage(file: VirtualFile, project: Project): String? {
        val dir = directoryOf(file) ?: return null
        val root = rootFor(dir, project) ?: return null
        return packageUnder(root.dir, dir)
    }

    /**
     * The packages a `.jux` file at [file] may declare, when the layout says
     * so for certain; `null` when it doesn't (no root, or only a
     * [Kind.FALLBACK] guess), which means "anything goes".
     *
     * A test file may also use its production package plus `.test`, the
     * convention §B.1.2 describes.
     */
    fun expectedPackages(file: VirtualFile, project: Project): List<String>? {
        val dir = directoryOf(file) ?: return null
        val root = rootFor(dir, project)?.takeIf { it.authoritative } ?: return null
        val pkg = packageUnder(root.dir, dir) ?: return null
        if (!root.tests) return listOf(pkg)
        return listOf(pkg, if (pkg.isEmpty()) "test" else "$pkg.test")
    }

    /**
     * Whether [file] is an **entry file a `[[bin]] path` names**, and so not a
     * member of the package tree at all (§B.1.1, ERRATA E103).
     *
     * The canonical multi-binary project of §B.15.2 puts additional entries
     * under `src/bin/`:
     *
     * ```
     * src/
     * ├── lib.jux                 # shared code
     * ├── main.jux                # primary binary
     * └── bin/
     *     ├── server.jux          # additional binary
     *     └── migrator.jux        # additional binary
     * ```
     *
     * §B.1.1's derivation read those as members of a package `bin`, so the
     * compiler demanded `package bin;` from both of them (`E0301`) -- a package
     * name no `import` could ever usefully name, because the `[[bin]] path` key
     * is how the file is found. E103 lifted the requirement: an entry file is a
     * program's entry point, and may be package-less wherever it sits under
     * `src/`. So the editor must not ask for the line either.
     *
     * Only the requirement is lifted, which is why this answers one question
     * and not two. A package the file DOES declare is still checked against its
     * directory, and the exemption is per FILE rather than per directory: an
     * ordinary source sitting beside an entry in `src/bin/` still declares
     * `package bin;`. The dotted form `[[bin]] main = "xss.it.Main"` states the
     * entry's package in the manifest, so a file named that way is not exempt
     * either, and only a `path` key is read here.
     */
    fun isBinEntryFile(file: VirtualFile, project: Project): Boolean {
        if (file.isDirectory) return false
        val manifestDir = manifestDirFor(file, project) ?: return false
        val manifest = manifestDir.findChild(MANIFEST)?.takeIf { !it.isDirectory } ?: return false
        // An open, edited manifest is read from its document so the answer
        // follows the keystroke that added the `[[bin]]`; otherwise from disk.
        // `getCachedDocument` never CREATES one, which keeps this off the cost
        // of loading a document per daemon pass.
        val text = try {
            FileDocumentManager.getInstance().getCachedDocument(manifest)?.text
                ?: VfsUtilCore.loadText(manifest)
        } catch (_: Throwable) {
            // An unreadable or binary manifest says nothing; the ordinary rule
            // applies. Never let a manifest read take the daemon pass down.
            return false
        }
        return binEntryPaths(text).any { manifestDir.findFileByRelativePath(it) == file }
    }

    /**
     * The `path` value of every `[[bin]]` table in a manifest, as written.
     *
     * Line-level and regex-based, like [dev.jux.intellij.toml.JuxTomlModel]:
     * `jux.toml` is read as plain text when the TOML plugin is absent, so the
     * editor cannot depend on a TOML PSI being there.
     */
    fun binEntryPaths(manifestText: String): List<String> {
        val lines = manifestText.lines()
        val header = Regex("""^\s*\[\[\s*bin\s*]]\s*$""")
        val pathKey = Regex("""^\s*path\s*=\s*"([^"]*)"""")
        val out = ArrayList<String>()
        var i = 0
        while (i < lines.size) {
            if (!header.matches(lines[i].substringBefore('#'))) { i++; continue }
            var j = i + 1
            while (j < lines.size && !lines[j].substringBefore('#').trim().startsWith("[")) {
                pathKey.find(lines[j].substringBefore('#'))?.let { out.add(it.groupValues[1]) }
                j++
            }
            i = j
        }
        return out
    }

    /** The directory of the nearest `jux.toml` above [file], inside the project. */
    private fun manifestDirFor(file: VirtualFile, project: Project): VirtualFile? {
        val index = ProjectFileIndex.getInstance(project)
        var current: VirtualFile? = directoryOf(file)
        var hops = 0
        while (current != null && hops < MAX_DEPTH && index.isInContent(current)) {
            if (current.findChild(MANIFEST)?.isDirectory == false) return current
            current = current.parent
            hops++
        }
        return null
    }

    /** The package root holding [file] (a file or a directory), by the rules above. */
    fun rootFor(file: VirtualFile, project: Project): Root? {
        val dir = directoryOf(file) ?: return null
        val index = ProjectFileIndex.getInstance(project)
        val sourceRoot = index.getSourceRootForFile(dir)

        // 1. A marked Jux root.
        if (sourceRoot != null) markedKind(sourceRoot, project)?.let { return Root(sourceRoot, it) }

        // 2. The nearest manifest.
        when (val found = manifestRoot(dir, index)) {
            is ManifestLookup.Found -> return found.root
            ManifestLookup.Outside -> return null
            ManifestLookup.None -> Unit
        }

        // 3. Any other root, as a template convenience.
        val fallback = sourceRoot ?: index.getContentRootForFile(dir) ?: return null
        return Root(fallback, Kind.FALLBACK)
    }

    /**
     * The directory for [pkg] under [root], created when [create] is true and
     * it doesn't exist yet. Must run in a write action when creating.
     */
    fun directoryFor(root: Root, pkg: String, create: Boolean): VirtualFile? {
        if (pkg.isEmpty()) return root.dir
        val relative = pkg.replace('.', '/')
        return if (create) VfsUtil.createDirectoryIfMissing(root.dir, relative) else root.dir.findFileByRelativePath(relative)
    }

    /** The dotted package of [dir] under [root], or `null` when [dir] is not below it. */
    fun packageUnder(root: VirtualFile, dir: VirtualFile): String? {
        if (root == dir) return ""
        val relative = VfsUtilCore.getRelativePath(dir, root, '/') ?: return null
        return relative.replace('/', '.')
    }

    // ---- rule 1 ---------------------------------------------------------

    /** The Jux kind [sourceRoot] is marked with, or `null` for any other kind of root. */
    private fun markedKind(sourceRoot: VirtualFile, project: Project): Kind? {
        val module = ModuleUtilCore.findModuleForFile(sourceRoot, project) ?: return null
        for (entry in ModuleRootManager.getInstance(module).contentEntries) {
            for (folder in entry.sourceFolders) {
                if (folder.file != sourceRoot) continue
                return when (folder.rootType) {
                    JuxSourceRootType.SOURCE -> Kind.MARKED_SOURCES
                    JuxSourceRootType.TEST_SOURCE -> Kind.MARKED_TESTS
                    else -> null
                }
            }
        }
        return null
    }

    // ---- rule 2 ---------------------------------------------------------

    private sealed interface ManifestLookup {
        /** A manifest owns the file and names its root. */
        data class Found(val root: Root) : ManifestLookup

        /** A manifest owns the file, but it sits outside `src/` and `test/`. */
        data object Outside : ManifestLookup

        /** No manifest above the file inside the project. */
        data object None : ManifestLookup
    }

    /** Walk up from [dir] to the nearest `jux.toml` inside the project's content. */
    private fun manifestRoot(dir: VirtualFile, index: ProjectFileIndex): ManifestLookup {
        var current: VirtualFile? = dir
        var hops = 0
        while (current != null && hops < MAX_DEPTH && index.isInContent(current)) {
            if (current.findChild(MANIFEST)?.isDirectory == false) return underManifest(current, dir)
            current = current.parent
            hops++
        }
        return ManifestLookup.None
    }

    /** Which root of the project at [manifestDir] holds [dir]. */
    private fun underManifest(manifestDir: VirtualFile, dir: VirtualFile): ManifestLookup {
        val src = manifestDir.findChild("src")?.takeIf { it.isDirectory }
        val test = manifestDir.findChild("test")?.takeIf { it.isDirectory }
        return when {
            src != null && VfsUtilCore.isAncestor(src, dir, false) -> ManifestLookup.Found(Root(src, Kind.MANIFEST_SOURCES))
            test != null && VfsUtilCore.isAncestor(test, dir, false) -> ManifestLookup.Found(Root(test, Kind.MANIFEST_TESTS))
            src == null -> ManifestLookup.Found(Root(manifestDir, Kind.MANIFEST_SOURCES))
            else -> ManifestLookup.Outside
        }
    }

    /** [file] itself when it is a directory, else its parent. */
    private fun directoryOf(file: VirtualFile): VirtualFile? = if (file.isDirectory) file else file.parent

    /** A guard against a pathological (cyclic, symlinked) tree. */
    private const val MAX_DEPTH = 256
}
