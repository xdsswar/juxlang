package dev.jux.intellij.lsp4ij

import com.intellij.ide.util.PropertiesComponent
import com.intellij.openapi.extensions.ExtensionPointName
import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerEnablementSupport
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.server.OSProcessStreamConnectionProvider
import com.redhat.devtools.lsp4ij.server.StreamConnectionProvider
import dev.jux.intellij.run.JuxLspCommandLine

/*
 * CLASSLOADING FIREWALL: this is the ONLY package allowed to import
 * `com.redhat.devtools.lsp4ij.*`, and its classes may be referenced ONLY from
 * `lsp4ij.xml` (which loads exclusively when the LSP4IJ plugin is installed).
 * Anything else would NoClassDefFoundError on IDEs without LSP4IJ — the same
 * discipline the `dev.jux.intellij.lsp` ↔ `lsp.xml` pair follows for the
 * native client.
 */

/**
 * The LSP4IJ fallback client for `juxc-lsp` — diagnostics/completion/hover on
 * IDEs **without** the native LSP API (IDEA CE, PyCharm CE, Android Studio,
 * pre-2025.2 free IDEs) when the free LSP4IJ plugin is installed.
 *
 * **Native wins.** When the IDE's built-in LSP client is present (the
 * `lsp.xml` gate module is installed), [isEnabled] returns `false` so only
 * ONE `juxc-lsp` ever runs; LSP4IJ then shows the server as disabled in its
 * Language Servers view rather than silently competing.
 */
class JuxLsp4ijServerFactory : LanguageServerFactory, LanguageServerEnablementSupport {

    override fun createConnectionProvider(project: Project): StreamConnectionProvider =
        JuxLsp4ijConnectionProvider()

    override fun isEnabled(project: Project): Boolean =
        !nativeLspActive() &&
            PropertiesComponent.getInstance(project).getBoolean(ENABLED_KEY, true)

    /** The user's toggle in LSP4IJ's Language Servers UI, persisted per project. */
    override fun setEnabled(enabled: Boolean, project: Project) {
        PropertiesComponent.getInstance(project).setValue(ENABLED_KEY, enabled, true)
    }

    companion object {
        private const val ENABLED_KEY = "dev.jux.lsp4ij.enabled"
        private const val JUX_PROVIDER = "dev.jux.intellij.lsp.JuxLspServerSupportProvider"

        /**
         * The native LSP provider EP, addressed by name only — public
         * [ExtensionPointName] API (the `ExtensionsArea` route is marked
         * internal on 2024.2). `extensionsIfPointIsRegistered` returns an
         * empty list when the EP doesn't exist (Community IDEs), without
         * loading any platform-LSP class.
         */
        private val NATIVE_LSP_EP: ExtensionPointName<Any> =
            ExtensionPointName.create("com.intellij.platform.lsp.serverSupportProvider")

        /**
         * True when the native LSP client is actually serving Jux — i.e. OUR
         * provider is registered on the `platform.lsp.serverSupportProvider`
         * extension point.
         *
         * Why not just "EP exists": on paid IDEs 2024.2–2025.1 the EP exists
         * but the `com.intellij.modules.lsp` gate module does NOT (it first
         * ships in 2025.2), so `lsp.xml` never loads there and the Jux
         * provider is absent. An EP-existence probe would make this fallback
         * stand down at the same time — leaving NO server at all on three
         * release lines. Checking for the registered provider keeps exactly
         * one client active everywhere.
         *
         * LOCKSTEP: the provider FQN mirrors `lsp.xml`'s `implementationClass`.
         */
        fun nativeLspActive(): Boolean = try {
            NATIVE_LSP_EP.extensionsIfPointIsRegistered
                .any { it.javaClass.name == JUX_PROVIDER }
        } catch (_: Throwable) {
            // On any API drift, prefer running this fallback over silence.
            false
        }
    }
}

/**
 * Launches `juxc-lsp` over stdio via the shared [JuxLspCommandLine]. On a
 * missing binary the process start fails and LSP4IJ reports it in its console
 * — never an IDE error dialog (mirrors the native client's behaviour).
 */
class JuxLsp4ijConnectionProvider : OSProcessStreamConnectionProvider() {
    init {
        commandLine = JuxLspCommandLine.create()
    }
}
