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
    /**
     * Test-only override of [isServing]; `null` asks the real clients.
     *
     * Without it the serving branch was unreachable in CI — the fixture never
     * starts a server, so `isServing` short-circuited to `false` and every test
     * exercised the fallback. That left the stand-down itself, and the two
     * surfaces that deliberately run THROUGH it (interpolation holes and the §P
     * property members), covered by nothing at all: the configuration most
     * users are actually in was the one configuration never tested.
     */
    @get:org.jetbrains.annotations.TestOnly
    @set:org.jetbrains.annotations.TestOnly
    var servingOverride: Boolean? = null

    fun isServing(project: Project): Boolean {
        val app = ApplicationManager.getApplication()
        if (app.isUnitTestMode) return servingOverride ?: false

        // Native client: only when OUR provider is registered AND a Jux server
        // session is actually up. (A registered Jux provider implies the
        // platform LSP classes exist, so touching JuxNativeLspStatus is safe.)
        val nativeRegistered = NATIVE_LSP_EP.extensionsIfPointIsRegistered
            .any { it.javaClass.name == JUX_NATIVE_PROVIDER }
        if (nativeRegistered && JuxNativeLspStatus.isActive(project)) return true

        // LSP4IJ path: registrations exist exactly when the plugin is installed
        // and enabled (and then our lsp4ij.xml loaded too). Require the user's
        // toggle on, a real juxc-lsp binary, AND a live session — presence is
        // not service. Without the last check a server that failed to start or
        // crashed still read as serving, and since the fallback completion and
        // the `juxc --check` annotator both stand down on this flag, the editor
        // went silent with no diagnostics and no completion at all.
        if (LSP4IJ_SERVER_EP.extensionsIfPointIsRegistered.isEmpty()) return false
        if (!JuxLspCommandLine.isResolvable()) return false
        if (!PropertiesComponent.getInstance(project).getBoolean(LSP4IJ_ENABLED_KEY, true)) {
            return false
        }
        return dev.jux.intellij.lsp4ij.JuxLsp4ijStatus.isActive(project)
    }

    /** Which client, if any, is answering for this project. */
    enum class Engine {
        /** The platform's own LSP client (IDEA Ultimate, or free mode 2025.2+). */
        NATIVE_LSP,

        /** The LSP4IJ plugin hosting the same `juxc-lsp` process. */
        LSP4IJ,

        /** No server: the plugin's own parser, inspections and completion. */
        FALLBACK,
    }

    /**
     * The engine currently serving [project] — what [isServing] decides, but
     * named, so the editor can say which one answered.
     *
     * Nothing used to surface this. When a server dies the only symptom is that
     * features quietly get worse, which is indistinguishable from the plugin
     * being bad; see [dev.jux.intellij.lsp.JuxEngineStatusBarWidget].
     */
    fun engine(project: Project): Engine {
        val app = ApplicationManager.getApplication()
        if (app.isUnitTestMode) {
            return if (servingOverride == true) Engine.NATIVE_LSP else Engine.FALLBACK
        }

        val nativeRegistered = NATIVE_LSP_EP.extensionsIfPointIsRegistered
            .any { it.javaClass.name == JUX_NATIVE_PROVIDER }
        if (nativeRegistered && JuxNativeLspStatus.isActive(project)) return Engine.NATIVE_LSP

        if (LSP4IJ_SERVER_EP.extensionsIfPointIsRegistered.isNotEmpty() &&
            JuxLspCommandLine.isResolvable() &&
            PropertiesComponent.getInstance(project).getBoolean(LSP4IJ_ENABLED_KEY, true) &&
            dev.jux.intellij.lsp4ij.JuxLsp4ijStatus.isActive(project)
        ) {
            return Engine.LSP4IJ
        }
        return Engine.FALLBACK
    }

    /**
     * Restart whichever `juxc-lsp` client is serving this project so it re-reads
     * `jux.toml` and rediscovers its dependencies — called when the manifest
     * changes (see `JuxManifestChangeListener`). Each backend is touched ONLY
     * behind its extension-point probe (same classloading discipline as
     * [isServing]): the firewalled restart helpers ([JuxNativeLspStatus.restart]
     * / [dev.jux.intellij.lsp4ij.JuxLsp4ijRestart]) link lazily at the guarded
     * call site, so neither loads on an IDE that lacks its LSP backend.
     */
    fun refresh(project: Project) {
        if (ApplicationManager.getApplication().isUnitTestMode) return

        val nativeRegistered = NATIVE_LSP_EP.extensionsIfPointIsRegistered
            .any { it.javaClass.name == JUX_NATIVE_PROVIDER }
        if (nativeRegistered) JuxNativeLspStatus.restart(project)

        if (LSP4IJ_SERVER_EP.extensionsIfPointIsRegistered.isNotEmpty()) {
            dev.jux.intellij.lsp4ij.JuxLsp4ijRestart.restart(project)
        }
    }
}
