package dev.jux.intellij.lsp4ij

import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerManager
import com.redhat.devtools.lsp4ij.ServerStatus

/*
 * CLASSLOADING FIREWALL: like every file in this package, it imports
 * `com.redhat.devtools.lsp4ij.*` and may be referenced ONLY behind an LSP4IJ
 * extension-point probe (see [dev.jux.intellij.lsp.JuxLspState]), so it never
 * links on an IDE without LSP4IJ.
 */

/**
 * Whether the LSP4IJ-hosted `juxc-lsp` session is actually **up**.
 *
 * The gate used to treat "LSP4IJ is installed, the toggle is on, and a
 * `juxc-lsp` binary resolves" as serving. None of that says a session exists.
 * A server that failed to start, or crashed, left `isServing` reporting true —
 * and since both the fallback completion and the `juxc --check` annotator stand
 * down on that flag, the editor went completely quiet: no completion, no
 * diagnostics, no explanation. The native client never had this problem because
 * it checks `state == Running`; this is the LSP4IJ counterpart.
 *
 * `starting` deliberately counts as serving, mirroring the native probe's
 * treatment of the startup window: during those seconds the server is about to
 * answer, and flapping the fallback in and out would duplicate items.
 */
object JuxLsp4ijStatus {
    /** LOCKSTEP: mirrors `lsp4ij.xml`'s `<server id="juxLanguageServer">`. */
    private const val SERVER_ID = "juxLanguageServer"

    fun isActive(project: Project): Boolean = try {
        when (LanguageServerManager.getInstance(project).getServerStatus(SERVER_ID)) {
            ServerStatus.started, ServerStatus.starting -> true
            else -> false
        }
    } catch (_: Throwable) {
        // API drift must never silence the editor: on any doubt, report NOT
        // serving so the fallbacks run. An extra completion source is a far
        // smaller failure than none at all.
        false
    }
}
