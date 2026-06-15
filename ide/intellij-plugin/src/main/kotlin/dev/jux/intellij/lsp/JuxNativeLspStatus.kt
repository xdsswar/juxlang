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
     * True only while a Jux LSP server is **Running** — i.e. actually able to
     * answer completion requests. `Initializing` deliberately does NOT count:
     * server start-up can take many seconds (toolchain spawn + project index),
     * and suppressing the fallback during that window left the user with NO
     * completion at all ("sometimes I don't get it"). Letting the IDE-side
     * fallback serve until the server is Running guarantees completion is
     * always available; the brief overlap as it flips to Running is harmless
     * versus a multi-second dead zone.
     */
    fun isActive(project: Project): Boolean = try {
        LspServerManager.getInstance(project)
            .getServersForProvider(JuxLspServerSupportProvider::class.java)
            .any { it.state == LspServerState.Running }
    } catch (_: Throwable) {
        // Any API drift or unexpected state must never break completion —
        // fall back to contributing the IDE-side items.
        false
    }

    /**
     * Restart every running Jux LSP server so `juxc-lsp` re-reads `jux.toml` and
     * re-resolves its `rust.<crate>` dependencies / stubs — invoked when the
     * manifest changes (see `JuxManifestChangeListener`). Guarded by the caller
     * behind the native-LSP EP probe; defensive here too so a restart hiccup
     * never surfaces as an IDE error.
     */
    fun restart(project: Project) {
        try {
            LspServerManager.getInstance(project)
                .stopAndRestartIfNeeded(JuxLspServerSupportProvider::class.java)
        } catch (_: Throwable) {
        }
    }
}
