package dev.jux.intellij.project

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.process.CapturingProcessHandler
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.WriteAction
import com.intellij.openapi.components.Service
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.progress.ProgressIndicator
import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.progress.Task
import com.intellij.openapi.project.Project
import com.intellij.openapi.roots.AdditionalLibraryRootsListener
import com.intellij.openapi.startup.ProjectActivity
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VfsUtilCore
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.newvfs.BulkFileListener
import com.intellij.openapi.vfs.newvfs.events.VFileEvent
import com.intellij.util.Alarm
import dev.jux.intellij.run.JuxToolchain
import org.jetbrains.annotations.TestOnly
import java.io.File
import java.util.concurrent.ConcurrentHashMap

/**
 * Generates the `.jux.d` stub of a Rust crate (or several) through the
 * toolchain. Replaceable in tests, where no toolchain runs.
 */
fun interface JuxStubGenerator {
    /** Produce the stubs of [deps]; [indicator] reports progress and cancels. */
    fun generate(project: Project, deps: List<JuxDependencies.ForeignDep>, indicator: ProgressIndicator)
}

/**
 * Keeps the IDE's view of the project's dependencies current.
 *
 * The code behind every dependency is indexed ([JuxDependencies],
 * [dev.jux.intellij.resolve.JuxLibraryRootsProvider]); this service reacts
 * when that code changes:
 *
 * - **A `jux.toml` changes** (the project's or a dependency's): the dependency
 *   set is re-read and the library roots re-announced, so a new path or git
 *   dependency is indexed and a removed one leaves the index. A Rust
 *   dependency whose entry changed (version, features, source) gets its stub
 *   regenerated.
 * - **A path dependency's or workspace member's sources change**: nothing to
 *   do here. Library roots over local directories are VFS-watched, so the
 *   platform re-indexes the edited file like any project file.
 * - **A git checkout changes** (`jux update`, a new ref fetched): its root is
 *   refreshed and re-announced.
 * - **A path-sourced Rust crate's sources change** (its `.rs` files or its
 *   `Cargo.toml`): its stub is regenerated.
 * - **A stub is missing or stale** (the toolchain's own header says it was
 *   generated from another source): regenerated when the project opens.
 *
 * Regeneration uses the toolchain's own cache rules: a stub the toolchain
 * would keep is left alone, and one whose inputs changed in a way its cache
 * key does not cover (a version or feature change, edited crate sources) is
 * removed so the toolchain's "absent means regenerate" rule applies.
 * Everything runs debounced, off the EDT, in a cancellable background task
 * with a progress indicator; typing never waits for it.
 */
@Service(Service.Level.PROJECT)
class JuxDependencySync(private val project: Project) : Disposable {

    private val alarm = Alarm(Alarm.ThreadToUse.POOLED_THREAD, this)

    /** The roots last announced to the platform, to report what changed. */
    @Volatile
    private var announcedRoots: List<VirtualFile> = emptyList()

    /** Each foreign dependency's last-seen manifest entry, by stub path. */
    private val fingerprints = ConcurrentHashMap<String, String>()

    /** Work queued by the listener and not yet run. */
    private val pendingManifest = java.util.concurrent.atomic.AtomicBoolean(false)
    private val pendingRoots = java.util.concurrent.atomic.AtomicBoolean(false)
    private val pendingRegeneration = ConcurrentHashMap.newKeySet<String>()

    /** Test hook: the stub generator to use instead of the toolchain. */
    @TestOnly
    @Volatile
    var generatorForTests: JuxStubGenerator? = null

    /**
     * Test hook: forget every remembered manifest entry and queued event.
     * Light tests share one project, so what one case's manifest said must
     * not read as a change in the next case's.
     */
    @TestOnly
    fun resetForTests() {
        fingerprints.clear()
        pendingManifest.set(false)
        pendingRoots.set(false)
        pendingRegeneration.clear()
        generatorForTests = null
    }

    // ------------------------------------------------------------ events

    /**
     * The paths events are routed against, refreshed off the EDT by [flush]:
     * VFS events arrive on the EDT under the write lock, so routing must not
     * compute the dependency snapshot there.
     */
    private data class WatchSet(
        val gitRoots: List<String> = emptyList(),
        val crateDirs: List<Pair<String, JuxDependencies.ForeignDep>> = emptyList(),
    )

    @Volatile
    private var watch = WatchSet()

    private fun refreshWatchSet(snapshot: JuxDependencies.Snapshot) {
        watch = WatchSet(
            gitRoots = snapshot.dependencyPackages.filter { it.origin == JuxDependencies.Origin.GIT }
                .map { it.root.path.lowercase().trimEnd('/') },
            // VFS paths, the form event paths arrive in (not canonical disk paths).
            crateDirs = snapshot.foreignDeps.mapNotNull { dep ->
                dep.crateDir()?.path?.trimEnd('/')?.lowercase()?.let { it to dep }
            },
        )
    }

    /** Route one batch of VFS events. Cheap: path checks only, work is deferred. */
    fun onFileEvents(events: List<VFileEvent>) {
        if (project.isDisposed) return
        val gitRoots = watch.gitRoots
        val crateDirs = watch.crateDirs
        for (event in events) {
            val path = event.path.replace('\\', '/')
            val lower = path.lowercase()
            when {
                lower.endsWith("/" + JuxDependencies.MANIFEST) && "/target/" !in lower -> pendingManifest.set(true)
                gitRoots.any { lower.startsWith("$it/") || lower == it } -> pendingRoots.set(true)
                else -> for ((dir, dep) in crateDirs) {
                    val isCrateSource = lower.startsWith("$dir/") &&
                        (lower.endsWith(".rs") || lower.endsWith("/cargo.toml"))
                    if (isCrateSource && "/target/" !in lower.removePrefix(dir)) pendingRegeneration.add(dep.stubPath)
                }
            }
        }
        if (pendingManifest.get() || pendingRoots.get() || pendingRegeneration.isNotEmpty()) schedule()
    }

    private fun schedule() {
        // Tests share one light project across cases; a timer firing into the
        // next case would re-announce roots under it. They call [flush].
        if (ApplicationManager.getApplication().isUnitTestMode) return
        alarm.cancelAllRequests()
        alarm.addRequest({ flush() }, DEBOUNCE_MS)
    }

    /** Run the queued work now: re-read manifests, re-announce roots, regenerate stubs. */
    fun flush() {
        if (project.isDisposed) return
        val manifestChanged = pendingManifest.getAndSet(false)
        val rootsChanged = pendingRoots.getAndSet(false)
        val regenerate = HashSet<String>().also { it.addAll(pendingRegeneration); pendingRegeneration.removeAll(it) }
        if (manifestChanged || rootsChanged) JuxDependencies.invalidate(project)
        val snapshot = JuxDependencies.snapshot(project)
        refreshWatchSet(snapshot)
        if (manifestChanged) regenerate.addAll(changedForeignDeps(snapshot))
        announceRoots()
        val toGenerate = snapshot.foreignDeps.filter { it.stubPath in regenerate }
        // An entry that changed in a way the stub's cache key does not cover
        // (version, features, edited crate sources): drop the stub so the
        // toolchain's own "absent means regenerate" rule applies.
        for (dep in toGenerate) deleteStub(dep)
        val stale = (toGenerate + snapshot.staleForeignDeps()).distinctBy { it.stubPath }
        if (stale.isNotEmpty()) generate(stale)
    }

    /** Foreign dependencies whose manifest entry differs from the one last seen. */
    private fun changedForeignDeps(snapshot: JuxDependencies.Snapshot): Set<String> {
        val out = HashSet<String>()
        for (dep in snapshot.foreignDeps) {
            val previous = fingerprints.put(dep.stubPath, dep.spec.fingerprint)
            if (previous != null && previous != dep.spec.fingerprint) out.add(dep.stubPath)
        }
        return out
    }

    /** Record every foreign dependency's entry as seen, without regenerating anything. */
    fun rememberCurrentEntries() {
        val snapshot = JuxDependencies.snapshot(project)
        refreshWatchSet(snapshot)
        for (dep in snapshot.foreignDeps) fingerprints.putIfAbsent(dep.stubPath, dep.spec.fingerprint)
    }

    // ------------------------------------------------------------ roots

    /**
     * Tell the platform the dependency roots changed, so new ones are indexed
     * and dropped ones leave the index. Runs under the write lock on the EDT,
     * as the platform requires.
     */
    fun announceRoots() {
        val now = JuxDependencies.snapshot(project).libraryRoots().values.flatten().filter { it.isValid }
        val before = announcedRoots
        if (now == before) return
        announcedRoots = now
        val fire = Runnable {
            if (project.isDisposed) return@Runnable
            WriteAction.run<RuntimeException> {
                AdditionalLibraryRootsListener.fireAdditionalLibraryChanged(
                    project, "Jux dependencies", before.filter { it.isValid }, now, "jux-dependencies",
                )
            }
        }
        val app = ApplicationManager.getApplication()
        if (app.isDispatchThread) fire.run() else app.invokeLater(fire, project.disposed)
    }

    // ------------------------------------------------------------ stubs

    private fun deleteStub(dep: JuxDependencies.ForeignDep) {
        val vf = dep.stubFile()
        if (vf != null) {
            // Through the VFS, so the index drops the old stub at once (and so
            // a test's in-memory file system is handled like the disk).
            val app = ApplicationManager.getApplication()
            val delete = Runnable {
                try {
                    WriteAction.run<Exception> { if (vf.isValid) vf.delete(this) }
                } catch (e: Exception) {
                    LOG.warn("could not remove stale stub ${vf.path}", e)
                }
            }
            if (app.isDispatchThread) delete.run() else app.invokeAndWait(delete)
            return
        }
        val file = File(dep.stubPath)
        if (file.isFile && !file.delete()) LOG.warn("could not remove stale stub ${file.path}")
    }

    private fun generate(deps: List<JuxDependencies.ForeignDep>) {
        val generator = generatorForTests
        if (generator != null) {
            // Tests: run inline so the effect is observable when flush returns.
            generator.generate(project, deps, com.intellij.openapi.progress.EmptyProgressIndicator())
            afterGeneration(deps)
            return
        }
        val task = object : Task.Backgroundable(project, "Generating Jux crate stubs", true) {
            override fun run(indicator: ProgressIndicator) {
                ToolchainStubGenerator.generate(project, deps, indicator)
            }

            override fun onFinished() = afterGeneration(deps)
        }
        ProgressManager.getInstance().run(task)
    }

    /** Make the regenerated stubs visible: refresh their directories and re-announce roots. */
    private fun afterGeneration(deps: List<JuxDependencies.ForeignDep>) {
        if (project.isDisposed) return
        val dirs = deps.map { File(it.stubPath).parentFile }.distinct()
        LocalFileSystem.getInstance().refreshIoFiles(dirs, true, true, null)
        JuxDependencies.invalidate(project)
        announceRoots()
    }

    override fun dispose() {}

    companion object {
        private val LOG = Logger.getInstance(JuxDependencySync::class.java)

        /** Coalesce a save's burst of events, and a quick run of edits, into one pass. */
        const val DEBOUNCE_MS = 800

        fun getInstance(project: Project): JuxDependencySync = project.getService(JuxDependencySync::class.java)
    }
}

/**
 * The toolchain as a stub generator: `jux --manifest-path <package> check`
 * resolves the package's foreign dependencies and writes each missing stub
 * into its `.jux-stubs/` (`juxc-driver`'s `resolve_and_load_stub_sources`),
 * the same code path a build takes. One run per package that owns a stale
 * stub; cancelling the indicator stops the process.
 */
object ToolchainStubGenerator : JuxStubGenerator {
    private val LOG = Logger.getInstance(ToolchainStubGenerator::class.java)

    override fun generate(project: Project, deps: List<JuxDependencies.ForeignDep>, indicator: ProgressIndicator) {
        val jux = JuxToolchain.find("jux") ?: run {
            LOG.info("no `jux` toolchain found; crate stubs stay as they are")
            return
        }
        val owners = deps.map { it.owner }.distinctBy { it.root.path }
        for ((i, owner) in owners.withIndex()) {
            indicator.checkCanceled()
            indicator.text = "Generating stubs for ${owner.name ?: owner.root.name}"
            indicator.fraction = i.toDouble() / owners.size
            val root = VfsUtilCore.virtualToIoFile(owner.root)
            val cmd = GeneralCommandLine(jux, "--manifest-path", root.path, "check")
                .withWorkDirectory(root)
                .withCharset(Charsets.UTF_8)
            try {
                val output = CapturingProcessHandler(cmd).runProcessWithProgressIndicator(indicator, GENERATION_TIMEOUT_MS)
                if (output.exitCode != 0) LOG.info("`jux check` in ${root.path} exited ${output.exitCode}: ${output.stderr.take(2000)}")
            } catch (e: Exception) {
                LOG.info("stub generation in ${root.path} failed", e)
            }
        }
    }

    private const val GENERATION_TIMEOUT_MS = 10 * 60 * 1000
}

/** Routes VFS events to [JuxDependencySync]. Registered as a project listener. */
class JuxDependencyFileListener(private val project: Project) : BulkFileListener {
    override fun after(events: MutableList<out VFileEvent>) {
        if (project.isDisposed) return
        JuxDependencySync.getInstance(project).onFileEvents(events)
    }
}

/** On open: learn the current entries and regenerate any stub that is missing or stale. */
class JuxDependencyStartup : ProjectActivity {
    override suspend fun execute(project: Project) {
        if (ApplicationManager.getApplication().isUnitTestMode) return
        val sync = JuxDependencySync.getInstance(project)
        sync.rememberCurrentEntries()
        sync.flush()
    }
}
