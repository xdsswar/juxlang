package dev.jux.intellij.lsp4ij

import com.intellij.ide.util.PropertiesComponent
import com.intellij.openapi.extensions.ExtensionPointName
import com.intellij.openapi.project.Project
import com.redhat.devtools.lsp4ij.LanguageServerEnablementSupport
import com.redhat.devtools.lsp4ij.LanguageServerFactory
import com.redhat.devtools.lsp4ij.client.LanguageClientImpl
import com.intellij.openapi.roots.ModuleRootEvent
import com.intellij.openapi.roots.ModuleRootListener
import dev.jux.intellij.project.JuxSourceRootsConfig
import java.util.Collections
import java.util.WeakHashMap
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

    /** A client whose settings carry `jux.sourceRoots` (§I.4). */
    override fun createLanguageClient(project: Project): LanguageClientImpl = JuxLsp4ijLanguageClient(project)

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

/**
 * The LSP4IJ client for `juxc-lsp`: its settings are `{ "jux": { "sourceRoots":
 * [...] } }`, which LSP4IJ serves for `workspace/configuration` requests and
 * sends with `workspace/didChangeConfiguration` (§I.4, [JuxSourceRootsConfig]).
 */
class JuxLsp4ijLanguageClient(project: Project) : LanguageClientImpl(project) {
    init {
        LIVE.add(this)
    }

    override fun createSettings(): Any =
        com.google.gson.Gson().toJsonTree(mapOf("jux" to mapOf("sourceRoots" to JuxSourceRootsConfig.compute(project))))

    companion object {
        /** The clients alive now, so a roots change can reach each. */
        private val LIVE: MutableSet<JuxLsp4ijLanguageClient> = Collections.synchronizedSet(Collections.newSetFromMap(WeakHashMap()))

        /** Re-send the settings of every client serving [project]. */
        fun rootsChanged(project: Project) {
            val clients = synchronized(LIVE) { LIVE.filter { it.project == project } }
            for (client in clients) {
                try {
                    client.triggerChangeConfiguration()
                } catch (_: Throwable) {
                    // A client whose server stopped has nothing to tell.
                }
            }
        }
    }
}

/**
 * Re-sends `jux.sourceRoots` to the LSP4IJ server when roots are marked or
 * unmarked. Referenced only from `lsp4ij.xml`.
 */
class JuxLsp4ijRootsListener(private val project: Project) : ModuleRootListener {
    override fun rootsChanged(event: ModuleRootEvent) {
        if (!project.isDisposed) JuxLsp4ijLanguageClient.rootsChanged(project)
    }
}
