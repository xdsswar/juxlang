package dev.jux.intellij.lsp

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.LspServerSupportProvider
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.run.JuxToolchain

/**
 * Wires `juxc-lsp` into the IDE's **native** LSP API (§I.6).
 *
 * Referenced only from `lsp.xml`, which loads exclusively when the
 * `com.intellij.modules.ultimate` module is present (the native LSP API lives
 * in paid IDEs). On IDEs without it, this class is never loaded — so it can't
 * crash a Community-only IDE.
 *
 * When a `.jux` file opens, the IDE starts (or reuses) one project-wide
 * `juxc-lsp` process. All semantic features — diagnostics, hover, completion,
 * auto-import — flow from that server; there is no IDE-side language logic.
 */
class JuxLspServerSupportProvider : LspServerSupportProvider {
    override fun fileOpened(
        project: Project,
        file: VirtualFile,
        serverStarter: LspServerSupportProvider.LspServerStarter,
    ) {
        // Defensive: a failure here must never surface as an IDE error.
        try {
            if (file.fileType == JuxFileType) {
                serverStarter.ensureServerStarted(JuxLspDescriptor(project))
            }
        } catch (e: Exception) {
            LOG.warn("Could not start juxc-lsp", e)
        }
    }

    companion object {
        private val LOG = Logger.getInstance(JuxLspServerSupportProvider::class.java)
    }
}

/**
 * Describes how to launch the Jux language server: the `juxc-lsp` binary,
 * resolved via [JuxToolchain] (`$JUX_HOME` → `PATH`). If it can't be found,
 * the LSP framework reports the server as failed-to-start in the IDE's
 * Language Servers tool window — it does not crash the IDE.
 */
class JuxLspDescriptor(project: Project) : ProjectWideLspServerDescriptor(project, "Jux") {
    override fun isSupportedFile(file: VirtualFile): Boolean = file.fileType == JuxFileType

    override fun createCommandLine(): GeneralCommandLine =
        GeneralCommandLine(JuxToolchain.resolveJuxcLsp())
}
