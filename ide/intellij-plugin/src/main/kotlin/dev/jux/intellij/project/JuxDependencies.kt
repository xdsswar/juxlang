package dev.jux.intellij.project

import com.intellij.openapi.application.ReadAction
import com.intellij.openapi.components.Service
import com.intellij.openapi.project.Project
import com.intellij.openapi.roots.ProjectRootManager
import com.intellij.openapi.util.ModificationTracker
import com.intellij.openapi.util.SimpleModificationTracker
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VfsUtilCore
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.psi.util.CachedValueProvider
import com.intellij.psi.util.CachedValuesManager
import dev.jux.intellij.toolwindow.JuxToml
import org.jetbrains.annotations.TestOnly
import java.io.File

/**
 * Everything a Jux project depends on, discovered from its manifests.
 *
 * The editor indexes the code behind every dependency so completion, go-to
 * and auto-import reach it the way they reach the project's own code. Nothing
 * here is a list of names: the dependencies are whatever the project's
 * `jux.toml` files say, read the way the compiler reads them (`juxc-driver`'s
 * `manifest.rs`, `git_deps.rs` and `stubs.rs`). `jux.toml` is the only
 * manifest; there is no `module.jux`.
 *
 * - **Project packages**: every `jux.toml` under the project's content roots,
 *   the workspace root and its members alike. Their sources are project
 *   content and already indexed.
 * - **Jux path dependencies** (`path = "../lib"`) and **git dependencies**
 *   (the checkout in `<JUX_HOME>/git/<stem>-<hash8>`, `~/.jux` by default):
 *   their sources become read-only library roots, shown under External
 *   Libraries, and their own manifests are followed, so a dependency's
 *   dependencies are indexed too.
 * - **Rust crates** (`rust.<crate>`) and vendored C/C++ bindings
 *   (`c.<lib>`, `cpp.<lib>`): the compiler writes their generated `.jux.d`
 *   stubs into the owning package's `.jux-stubs/<kind>/`. A stub that is
 *   missing, or was generated from a different source than the manifest now
 *   names, is reported so [JuxDependencySync] can have the toolchain
 *   regenerate it.
 */
object JuxDependencies {

    /** Where a package came from. */
    enum class Origin { PROJECT, PATH, GIT }

    /** One Jux package: a directory with a `jux.toml`. */
    data class JuxPackage(
        val root: VirtualFile,
        val manifest: VirtualFile,
        val name: String?,
        val origin: Origin,
        /** The dependency entry that brought it in (`null` for project packages). */
        val via: String?,
    ) {
        /** The directories whose `.jux` files are this package's code. */
        fun sourceRoots(): List<VirtualFile> {
            val src = root.findChild("src")
            return if (src != null && src.isDirectory) listOf(src) else listOf(root)
        }

        /** The package's generated foreign stubs (`.jux-stubs/`), when it has any. */
        fun stubDir(): VirtualFile? = root.findChild(STUB_DIRNAME)?.takeIf { it.isDirectory }
    }

    /** A `rust.<crate>` / `c.<lib>` / `cpp.<lib>` dependency of one package. */
    data class ForeignDep(
        val owner: JuxPackage,
        val spec: JuxToml.DepSpec,
        /** `rust`, `c` or `cpp`. */
        val kind: String,
        /** The name after the prefix: `rust.tiny_skia` gives `tiny_skia`. */
        val name: String,
    ) {
        /** Where the compiler writes (or looks for) this dependency's stub, as a VFS path. */
        val stubPath: String
            get() = owner.root.path.trimEnd('/') + "/$STUB_DIRNAME/$kind/$name$STUB_EXT"

        /** The stub file, when it exists. */
        fun stubFile(): VirtualFile? = owner.root.findFileByRelativePath("$STUB_DIRNAME/$kind/$name$STUB_EXT")

        /**
         * The directory of a path-sourced Rust crate
         * (`rust.foo = { path = "../foo" }`), whose Rust sources the stub is
         * generated from; `null` for registry and git crates, or one not on disk.
         */
        fun crateDir(): VirtualFile? = spec.path?.let { resolveDir(owner.root, it) }

        /** [crateDir]'s path, normalized for comparing event paths against. */
        fun crateDirPath(): String? = crateDir()?.let { normalizedPath(it) }

        /**
         * The source tag the toolchain folds into a generated stub's first line
         * (`// juxc crate stub cache-version N source <tag>`, `stubs.rs`):
         * `registry`, `path:<absolute path>` or `git:<url>[#<ref>]`.
         */
        fun expectedSourceTag(): String = when {
            spec.path != null -> "path:" + (crateDir()?.let { normalizedPath(it) } ?: spec.path)
            spec.git != null -> "git:" + spec.git + (spec.gitRef?.let { "#$it" } ?: "")
            else -> "registry"
        }
    }

    /** What the project depends on, right now. */
    data class Snapshot(
        val projectPackages: List<JuxPackage>,
        /** Jux packages outside the project: path and git dependencies, transitively. */
        val dependencyPackages: List<JuxPackage>,
        val foreignDeps: List<ForeignDep>,
    ) {
        /** Every root that must be indexed as a library, grouped by the package it belongs to. */
        fun libraryRoots(): Map<JuxPackage, List<VirtualFile>> {
            val out = LinkedHashMap<JuxPackage, List<VirtualFile>>()
            for (pkg in dependencyPackages) {
                out[pkg] = (pkg.sourceRoots() + listOfNotNull(pkg.stubDir())).distinct()
            }
            return out
        }

        /** Foreign dependencies whose stub is missing or was generated from another source. */
        fun staleForeignDeps(): List<ForeignDep> = foreignDeps.filter { dep ->
            // Hand-vendored C/C++ bindings carry no marker; only a missing one matters.
            val file = dep.stubFile() ?: return@filter dep.kind == "rust"
            dep.kind == "rust" && !stubMatchesSource(file, dep)
        }
    }

    /** The current snapshot, cached until a manifest or the file tree changes. */
    fun snapshot(project: Project): Snapshot =
        CachedValuesManager.getManager(project).getCachedValue(project) {
            CachedValueProvider.Result.create(
                compute(project),
                tracker(project),
                VirtualFileManager.VFS_STRUCTURE_MODIFICATIONS,
            )
        }

    /** Bumped whenever a manifest's content changes, so the snapshot is recomputed. */
    fun tracker(project: Project): ModificationTracker = project.getService(Tracker::class.java).tracker

    /** Invalidate the cached snapshot (a manifest changed, a stub was regenerated). */
    fun invalidate(project: Project) {
        project.getService(Tracker::class.java).tracker.incModificationCount()
    }

    @Service(Service.Level.PROJECT)
    class Tracker {
        val tracker = SimpleModificationTracker()
    }

    // ------------------------------------------------------------ discovery

    private fun compute(project: Project): Snapshot = ReadAction.compute<Snapshot, RuntimeException> {
        val content = ProjectRootManager.getInstance(project).contentRoots.filter { it.isValid }
        val projectManifests = LinkedHashSet<VirtualFile>()
        for (root in content) collectManifests(root, projectManifests, depth = 0)

        val projectPackages = projectManifests.map { packageOf(it, Origin.PROJECT, via = null) }
        val deps = LinkedHashMap<String, JuxPackage>() // by canonical root path
        val foreign = ArrayList<ForeignDep>()
        val queue = ArrayDeque(projectPackages)
        val visited = HashSet<String>()
        while (queue.isNotEmpty()) {
            val pkg = queue.removeFirst()
            if (!visited.add(pkg.root.path)) continue
            val text = readText(pkg.manifest)
            for (spec in JuxToml.dependencySpecs(text)) {
                val kind = FOREIGN_KINDS.firstOrNull { spec.name.startsWith("$it.") }
                if (kind != null) {
                    foreign.add(ForeignDep(pkg, spec, kind, spec.name.removePrefix("$kind.")))
                    continue
                }
                val depRoot = juxPackageRoot(pkg, spec) ?: continue
                if (isUnder(depRoot, content)) continue // a workspace sibling: already project code
                val manifest = depRoot.findChild(MANIFEST) ?: continue
                val origin = if (spec.path != null) Origin.PATH else Origin.GIT
                val dep = packageOf(manifest, origin, via = spec.name)
                if (deps.putIfAbsent(depRoot.path, dep) == null) queue.add(dep)
            }
        }
        Snapshot(projectPackages, deps.values.toList(), foreign)
    }

    /** The directory a Jux (not foreign) dependency's code lives in, if it exists on disk. */
    private fun juxPackageRoot(owner: JuxPackage, spec: JuxToml.DepSpec): VirtualFile? {
        spec.path?.let { p -> return resolveDir(owner.root, p) }
        spec.git?.let { url -> return gitCacheDir(url, spec.gitRef) }
        return null // a registry dependency: no local copy until a registry client exists
    }

    private fun packageOf(manifest: VirtualFile, origin: Origin, via: String?): JuxPackage =
        JuxPackage(manifest.parent, manifest, JuxToml.packageName(readText(manifest)), origin, via)

    /** Every `jux.toml` under [dir], skipping build output and tool directories. */
    private fun collectManifests(dir: VirtualFile, out: MutableSet<VirtualFile>, depth: Int) {
        if (depth > MAX_DEPTH || !dir.isDirectory) return
        dir.findChild(MANIFEST)?.takeIf { !it.isDirectory }?.let(out::add)
        for (child in dir.children) {
            if (child.isDirectory && child.name !in SKIP_DIRS) collectManifests(child, out, depth + 1)
        }
    }

    private fun resolveDir(base: VirtualFile, path: String): VirtualFile? {
        val f = File(path)
        if (f.isAbsolute) return LocalFileSystem.getInstance().findFileByIoFile(f)?.takeIf { it.isDirectory }
        return base.findFileByRelativePath(path.replace('\\', '/'))?.takeIf { it.isDirectory }
    }

    private fun isUnder(file: VirtualFile, roots: List<VirtualFile>): Boolean =
        roots.any { VfsUtilCore.isAncestor(it, file, false) }

    private fun readText(file: VirtualFile): String = try {
        VfsUtilCore.loadText(file)
    } catch (_: Exception) {
        ""
    }

    // ------------------------------------------------------------ git cache

    /** Test hook: where git dependency checkouts live instead of `<JUX_HOME>/git`. */
    @TestOnly
    @Volatile
    var gitCacheRootForTests: VirtualFile? = null

    /**
     * The checkout of a git dependency, `<JUX_HOME>/git/<stem>-<hash8>`, as
     * `juxc-driver`'s `git_dep_cache_dir` names it; `null` until it has been
     * fetched (a build or `jux update` fetches it).
     */
    fun gitCacheDir(url: String, gitRef: String?): VirtualFile? {
        val dirName = gitCacheDirName(url, gitRef)
        gitCacheRootForTests?.let { return it.findChild(dirName)?.takeIf { d -> d.isDirectory } }
        val root = File(juxHome() ?: return null, "git")
        return LocalFileSystem.getInstance().findFileByIoFile(File(root, dirName))?.takeIf { it.isDirectory }
    }

    /** The checkout directory's name: the repo stem and the low 32 bits of FNV-1a over `url[#ref]`. */
    fun gitCacheDirName(url: String, gitRef: String?): String {
        val key = if (gitRef != null) "$url#$gitRef" else url
        val hash = fnv1a64(key).toInt().toUInt()
        return "${repoStem(url)}-" + String.format("%08x", hash.toLong())
    }

    /** `https://github.com/u/my-lib.git` gives `my-lib`, sanitized for a directory name. */
    fun repoStem(url: String): String {
        val tail = url.trimEnd('/').split('/', ':').lastOrNull() ?: "dep"
        val bare = tail.removeSuffix(".git")
        val cleaned = bare.map { if (it.isLetterOrDigit() && it.code < 128 || it == '-' || it == '_') it else '_' }
            .joinToString("")
        return cleaned.ifEmpty { "dep" }
    }

    /** 64-bit FNV-1a, the stable hash the driver keys its git cache with. */
    fun fnv1a64(s: String): Long {
        var hash = -0x340d631b7bdddcdbL // 0xcbf29ce484222325
        for (b in s.toByteArray(Charsets.UTF_8)) {
            hash = hash xor (b.toLong() and 0xff)
            hash *= 0x100000001b3L
        }
        return hash
    }

    /** `$JUX_HOME`, else `~/.jux` (the driver's `jux_home`). */
    fun juxHome(): File? {
        val env = System.getenv("JUX_HOME")?.takeIf { it.isNotBlank() }
        if (env != null) return File(env)
        val home = (System.getenv("USERPROFILE") ?: System.getenv("HOME"))?.takeIf { it.isNotBlank() } ?: return null
        return File(home, ".jux")
    }

    // ------------------------------------------------------------ stub freshness

    /**
     * True when [stub]'s header names the source [dep] now has. The header is
     * the toolchain's own cache key, so this reads its answer rather than
     * keeping a second scheme: a mismatch means the toolchain would regenerate.
     */
    fun stubMatchesSource(stub: VirtualFile, dep: ForeignDep): Boolean {
        val first = try {
            VfsUtilCore.loadText(stub).lineSequence().firstOrNull().orEmpty()
        } catch (_: Exception) {
            return false
        }
        val tag = first.substringAfter(" source ", missingDelimiterValue = "").trim()
        if (tag.isEmpty()) return false
        val expected = dep.expectedSourceTag()
        if (tag == expected) return true
        if (tag.startsWith("path:") && expected.startsWith("path:")) {
            return normalizedPath(tag.removePrefix("path:")) == expected.removePrefix("path:")
        }
        return tag == expected
    }

    /**
     * A path in one comparable form: `/` separators, no trailing slash,
     * canonical for a local file, lower-cased where the file system ignores
     * case (Windows).
     */
    fun normalizedPath(file: VirtualFile): String =
        // Only a real disk path has a canonical form. A test's in-memory
        // `temp://` file system also extends LocalFileSystem, but its paths
        // (`/libs/x`) mean nothing to java.io.File.
        if (file.fileSystem.protocol == com.intellij.openapi.vfs.StandardFileSystems.FILE_PROTOCOL) {
            normalizedPath(file.path)
        } else {
            file.path.trimEnd('/')
        }

    /** [normalizedPath] for a path string (a local path as the toolchain wrote it). */
    fun normalizedPath(path: String): String {
        val canonical = try {
            File(path).canonicalPath
        } catch (_: Exception) {
            path
        }.replace('\\', '/').trimEnd('/')
        return if (File.separatorChar == '\\') canonical.lowercase() else canonical
    }

    const val MANIFEST = "jux.toml"
    const val STUB_DIRNAME = ".jux-stubs"
    const val STUB_EXT = ".jux.d"
    private val FOREIGN_KINDS = listOf("rust", "cpp", "c")
    private val SKIP_DIRS = setOf("target", ".git", ".idea", "node_modules", "build", STUB_DIRNAME, ".gradle")
    private const val MAX_DEPTH = 6
}
