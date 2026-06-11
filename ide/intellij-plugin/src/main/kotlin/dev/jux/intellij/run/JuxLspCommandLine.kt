package dev.jux.intellij.run

import com.intellij.execution.configurations.GeneralCommandLine

/**
 * Single source of truth for launching `juxc-lsp` — shared by the IDE's
 * **native** LSP client (`lsp.xml` → `dev.jux.intellij.lsp`) and the
 * **LSP4IJ** fallback client (`lsp4ij.xml` → `dev.jux.intellij.lsp4ij`), so
 * server arguments/environment can never drift between the two paths.
 *
 * Platform-only imports — safe to load in every IDE.
 */
object JuxLspCommandLine {
    /**
     * The launch command. Never throws: when no real binary is found,
     * [JuxToolchain.resolveJuxcLsp] degrades to the bare name `juxc-lsp`, and
     * the client reports a start failure in its own console instead of
     * surfacing an IDE error.
     */
    fun create(): GeneralCommandLine = GeneralCommandLine(JuxToolchain.resolveJuxcLsp())

    /** True when a real `juxc-lsp` executable was located (vs the bare-name fallback). */
    fun isResolvable(): Boolean = JuxToolchain.find("juxc-lsp") != null
}
