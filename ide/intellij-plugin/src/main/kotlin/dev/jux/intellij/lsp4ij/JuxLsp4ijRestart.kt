package dev.jux.intellij.lsp4ij

import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerManager

/*
 * CLASSLOADING FIREWALL: like every file in this package, it imports
 * `com.redhat.devtools.lsp4ij.*` and may be referenced ONLY behind an LSP4IJ
 * extension-point probe (see [dev.jux.intellij.lsp.JuxLspState.refresh]), so it
 * never links on an IDE without LSP4IJ.
 */

/**
 * Restarts the LSP4IJ-hosted `juxc-lsp` so the server re-reads `jux.toml` and
 * re-resolves its dependencies after the manifest changes. Stop-then-start is
 * LSP4IJ's restart idiom (there is no single restart call); the id mirrors
 * `lsp4ij.xml`'s `<server id="juxLanguageServer">`.
 */
object JuxLsp4ijRestart {
    private const val SERVER_ID = "juxLanguageServer"

    fun restart(project: Project) {
        try {
            val mgr = LanguageServerManager.getInstance(project)
            mgr.stop(SERVER_ID)
            mgr.start(SERVER_ID)
        } catch (_: Throwable) {
        }
    }
}
