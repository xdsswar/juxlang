package dev.jux.intellij.lsp

import com.intellij.codeInsight.daemon.DaemonCodeAnalyzer
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.newvfs.BulkFileListener
import com.intellij.openapi.vfs.newvfs.events.VFileEvent
import com.intellij.util.Alarm

/**
 * Re-discovers project dependencies when **`jux.toml`** changes. When the user
 * adds or edits a dependency and the manifest is written to disk, this restarts
 * the serving `juxc-lsp` (via [JuxLspState.refresh]) so the new `rust.<crate>`
 * deps and their stubs are resolved, then kicks the daemon to refresh
 * highlighting and inspections against the new project model.
 *
 * Registered as a project-level `BulkFileListener` (see `plugin.xml`). VFS
 * change events fire on save/sync (not per keystroke), and a short debounce
 * coalesces the burst a single save produces — and rapid successive edits — into
 * one restart. Touches no LSP classes directly (the firewalled restart lives
 * behind [JuxLspState.refresh]'s EP probes), so it loads safely on every IDE.
 */
class JuxManifestChangeListener(private val project: Project) : BulkFileListener {
    // Restart on the EDT (LSP managers + the daemon require it); parented to the
    // project so it's disposed with it.
    private val alarm = Alarm(Alarm.ThreadToUse.SWING_THREAD, project)

    override fun after(events: MutableList<out VFileEvent>) {
        if (project.isDisposed) return
        val base = project.basePath ?: return
        if (events.none { isManifestPath(it.path, base) }) return
        LOG.info("jux.toml changed - scheduling dependency re-discovery (LSP restart)")
        alarm.cancelAllRequests()
        alarm.addRequest({ rediscover() }, REFRESH_DELAY_MS)
    }

    private fun rediscover() {
        if (project.isDisposed) return
        LOG.info("Re-discovering Jux dependencies: restarting juxc-lsp + refreshing daemon")
        JuxLspState.refresh(project)
        DaemonCodeAnalyzer.getInstance(project).restart("jux.toml dependencies changed")
    }

    internal companion object {
        private val LOG = Logger.getInstance(JuxManifestChangeListener::class.java)

        /** Coalesce a save's event burst (and rapid edits) into one restart. */
        const val REFRESH_DELAY_MS = 800

        /**
         * True when [path] is a `jux.toml` inside the project rooted at [base]
         * (the workspace root or any member). Both are normalized to `/` so it
         * matches regardless of the OS path separator.
         */
        fun isManifestPath(path: String, base: String): Boolean {
            val p = path.replace('\\', '/')
            val b = base.replace('\\', '/')
            return p.endsWith("/jux.toml") && p.startsWith(b)
        }
    }
}
