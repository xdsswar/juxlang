package dev.jux.intellij.lsp

import com.intellij.ide.util.PropertiesComponent
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.extensions.ExtensionPointName
import com.intellij.openapi.project.Project
import dev.jux.intellij.run.JuxLspCommandLine

/**
 * Single source of truth for "is an LSP client actively serving `juxc-lsp` for
 * this project?". Both the fallback completion ([dev.jux.intellij.completion.JuxCompletionContributor])
 * and the on-demand semantic annotator ([dev.jux.intellij.highlight.JuxSemanticAnnotator])
 * stand down when this is true, so their output never duplicates what the
 * language server already publishes.
 *
 * Classloading discipline (kept from the original completion gate): the native
 * check goes through [JuxNativeLspStatus] ONLY behind the
 * `platform.lsp.serverSupportProvider` extension-point probe (those classes
 * exist exactly when the EP does); the LSP4IJ check never touches LSP4IJ
 * classes — plugin presence (its `server` EP has registrations) plus the
 * persisted enable flag and a resolvable toolchain are enough.
 */
object JuxLspState {
    /** Public LSP EP — `extensionsIfPointIsRegistered` is empty when absent. */
    private val NATIVE_LSP_EP: ExtensionPointName<Any> =
        ExtensionPointName.create("com.intellij.platform.lsp.serverSupportProvider")
    private val LSP4IJ_SERVER_EP: ExtensionPointName<Any> =
        ExtensionPointName.create("com.redhat.devtools.lsp4ij.server")

    /** LOCKSTEP: mirrors `lsp.xml`'s `implementationClass`. */
    private const val JUX_NATIVE_PROVIDER = "dev.jux.intellij.lsp.JuxLspServerSupportProvider"

    /** LOCKSTEP: mirrors JuxLsp4ijServerFactory.ENABLED_KEY (classloading firewall). */
    private const val LSP4IJ_ENABLED_KEY = "dev.jux.lsp4ij.enabled"

    /**
     * True when `juxc-lsp` is actually serving this project (native client up,
     * or LSP4IJ installed+enabled with a resolvable toolchain). Returns false in
     * unit-test mode (no server is ever started there), so IDE-side fallbacks
     * stay exercised by the test fixture.
     */
    fun isServing(project: Project): Boolean {
        val app = ApplicationManager.getApplication()
        if (app.isUnitTestMode) return false

        // Native client: only when OUR provider is registered AND a Jux server
        // session is actually up. (A registered Jux provider implies the
        // platform LSP classes exist, so touching JuxNativeLspStatus is safe.)
        val nativeRegistered = NATIVE_LSP_EP.extensionsIfPointIsRegistered
            .any { it.javaClass.name == JUX_NATIVE_PROVIDER }
        if (nativeRegistered && JuxNativeLspStatus.isActive(project)) return true

        // LSP4IJ path: registrations exist exactly when the plugin is installed
        // and enabled (and then our lsp4ij.xml loaded too). Require the user's
        // toggle on AND a real juxc-lsp binary.
        if (LSP4IJ_SERVER_EP.extensionsIfPointIsRegistered.isEmpty()) return false
        if (!JuxLspCommandLine.isResolvable()) return false
        return PropertiesComponent.getInstance(project).getBoolean(LSP4IJ_ENABLED_KEY, true)
    }
}
