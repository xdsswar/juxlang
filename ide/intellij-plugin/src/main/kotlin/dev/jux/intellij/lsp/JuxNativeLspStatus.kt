package dev.jux.intellij.lsp

import com.intellij.openapi.project.Project
import com.intellij.platform.lsp.api.LspServerManager
import com.intellij.platform.lsp.api.LspServerState

/**
 * Read-only probe: is the native `juxc-lsp` session up (or coming up) for
 * this project?
 *
 * Used by [dev.jux.intellij.completion.JuxCompletionContributor] to suppress
 * its IDE-side fallback items (keywords + in-file names) when the server's
 * scope-aware completions will arrive anyway — without the gate the two lists
 * merge and the fallback's flat names outrank the server's ranked ones.
 *
 * CLASSLOADING: this file imports `com.intellij.platform.lsp.api.*`, which
 * exists only when the `com.intellij.modules.lsp` module is present. Callers
 * MUST probe `hasExtensionPoint("com.intellij.platform.lsp.serverSupportProvider")`
 * before touching this object (same lockstep rule as
 * `JuxLsp4ijServerFactory.nativeLspActive`).
 */
object JuxNativeLspStatus {
    /**
     * True while a Jux LSP server is initializing or running. `Initializing`
     * counts as active so the fallback doesn't flash a duplicate list during
     * the server's first second.
     */
    fun isActive(project: Project): Boolean = try {
        LspServerManager.getInstance(project)
            .getServersForProvider(JuxLspServerSupportProvider::class.java)
            .any { it.state == LspServerState.Initializing || it.state == LspServerState.Running }
    } catch (_: Throwable) {
        // Any API drift or unexpected state must never break completion —
        // fall back to contributing the IDE-side items.
        false
    }
}
