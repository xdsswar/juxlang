package dev.jux.intellij.run

import java.io.File

/**
 * Locates the Jux command-line tools (`juxc`, `juxc-lsp`) for the IDE.
 *
 * Resolution order (first hit wins) for any tool:
 *  1. An explicit path passed in (if it names a real file).
 *  2. `$JUX_HOME/bin/<tool>[.exe]`, then `$JUX_HOME/<tool>[.exe]`.
 *  3. The bare command name, resolved on `PATH` by the OS.
 *
 * Recommended setup: install jux/juxc and set the **`JUX_HOME`** environment
 * variable to the install root — both the Run action and the LSP client find
 * the tools from there with no per-project configuration. Every method is
 * exception-safe and never throws.
 */
object JuxToolchain {
    /** Environment variable naming the Jux install root. */
    const val HOME_ENV = "JUX_HOME"

    /** Resolve the `juxc` compiler executable. */
    fun resolveJuxc(override: String? = null): String = resolve("juxc", override)

    /** Resolve the `juxc-lsp` language-server executable. */
    fun resolveJuxcLsp(override: String? = null): String = resolve("juxc-lsp", override)

    /** True if `JUX_HOME` is set and contains a `juxc` executable. */
    fun homeHasJuxc(): Boolean = fromHome("juxc") != null

    private fun resolve(base: String, override: String?): String {
        // 1. Explicit, existing file wins.
        if (!override.isNullOrBlank() && override != base) {
            try {
                val f = File(override)
                if (f.isFile) return f.absolutePath
            } catch (_: Exception) {
                // ignore and fall through to discovery
            }
        }
        // 2. $JUX_HOME.
        fromHome(base)?.let { return it }
        // 3. PATH fallback (or the override as a bare command name).
        return override?.takeIf { it.isNotBlank() } ?: base
    }

    private fun fromHome(base: String): String? {
        val home = try {
            System.getenv(HOME_ENV)?.trim().orEmpty()
        } catch (_: Exception) {
            return null
        }
        if (home.isEmpty()) return null
        val exe = exeName(base)
        for (candidate in listOf(File(File(home, "bin"), exe), File(home, exe))) {
            try {
                if (candidate.isFile) return candidate.absolutePath
            } catch (_: Exception) {
                // ignore unreadable candidate
            }
        }
        return null
    }

    private fun exeName(base: String): String {
        val win = try {
            System.getProperty("os.name").orEmpty().lowercase().contains("win")
        } catch (_: Exception) {
            false
        }
        return if (win) "$base.exe" else base
    }
}
