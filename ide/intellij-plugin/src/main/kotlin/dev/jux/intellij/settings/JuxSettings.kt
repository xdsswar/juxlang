package dev.jux.intellij.settings

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.util.xmlb.XmlSerializerUtil

/**
 * Application-level, persisted Jux toolchain configuration — the single place
 * the IDE records where the Jux command-line tools live, so every feature (the
 * LSP client, Run, the Jux tool window) resolves the same `juxc` / `juxc-lsp`
 * without per-project setup. Edited from **Settings | Tools | Jux Toolchain**
 * ([`JuxConfigurable`]).
 *
 * An empty [State.toolchainHome] means "auto-detect" — [dev.jux.intellij.run.JuxToolchain]
 * then falls back to `$JUX_HOME`, the system `PATH`, and the usual install
 * locations.
 */
@State(name = "JuxSettings", storages = [Storage("jux.xml")])
class JuxSettings : PersistentStateComponent<JuxSettings.State> {
    /** Serialized fields. */
    class State {
        /**
         * The Jux install root (the dir that holds `bin/juxc`, or `juxc`
         * directly), OR a direct path to the `juxc` executable. Empty = auto.
         */
        @JvmField
        var toolchainHome: String = ""

        /**
         * Default cross-compile target triple passed to `jux build --target`
         * from the Jux Project tool window. Empty = build for the host. Set via
         * the tool window's "Target" toolbar action; overrides any `[build]
         * target` declared in the project's `jux.toml`.
         */
        @JvmField
        var crossTarget: String = ""
    }

    private var state = State()

    override fun getState(): State = state
    override fun loadState(s: State) = XmlSerializerUtil.copyBean(s, state)

    var toolchainHome: String
        get() = state.toolchainHome
        set(value) {
            state.toolchainHome = value.trim()
        }

    /** Default `--target` triple for tool-window builds; blank = host. */
    var crossTarget: String
        get() = state.crossTarget
        set(value) {
            state.crossTarget = value.trim()
        }

    companion object {
        @JvmStatic
        fun getInstance(): JuxSettings =
            ApplicationManager.getApplication().getService(JuxSettings::class.java)
    }
}
