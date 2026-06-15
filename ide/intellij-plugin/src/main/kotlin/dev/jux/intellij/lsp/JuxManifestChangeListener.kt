package dev.jux.intellij.lsp

import com.intellij.codeInsight.daemon.DaemonCodeAnalyzer
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
        val base = project.basePath?.replace('\\', '/') ?: return
        val touched = events.any { e ->
            val p = e.path.replace('\\', '/')
            p.endsWith("/jux.toml") && p.startsWith(base)
        }
        if (!touched) return
        alarm.cancelAllRequests()
        alarm.addRequest({ rediscover() }, REFRESH_DELAY_MS)
    }

    private fun rediscover() {
        if (project.isDisposed) return
        JuxLspState.refresh(project)
        DaemonCodeAnalyzer.getInstance(project).restart("jux.toml dependencies changed")
    }

    private companion object {
        /** Coalesce a save's event burst (and rapid edits) into one restart. */
        const val REFRESH_DELAY_MS = 800
    }
}
