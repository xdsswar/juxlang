package dev.jux.intellij.run

import dev.jux.intellij.settings.JuxSettings
import java.io.File

/**
 * Locates the Jux command-line tools (`juxc`, `juxc-lsp`, `jux`) for the IDE.
 *
 * Resolution order (first hit wins) for any tool:
 *  1. An explicit path passed in (if it names a real file).
 *  2. The configured toolchain home (**Settings | Tools | Jux Toolchain**,
 *     [JuxSettings.toolchainHome]) — a direct `juxc` path, or an install root
 *     holding `bin/<tool>` / `<tool>`.
 *  3. `$JUX_HOME/bin/<tool>[.exe]`, then `$JUX_HOME/<tool>[.exe]`.
 *  4. Each directory on the system `PATH`.
 *  5. Common install locations (`~/.jux`, `~/.cargo/bin`, `/usr/local/bin`, …).
 *  6. The bare command name (so the OS still resolves it on `PATH`).
 *
 * So a user only needs ONE of: a configured path, `$JUX_HOME`, the tools on
 * `PATH`, or a standard install — and every IDE feature finds them. Every
 * method is exception-safe and never throws.
 */
object JuxToolchain {
    /** Environment variable naming the Jux install root. */
    const val HOME_ENV = "JUX_HOME"

    /** Resolve the `juxc` compiler executable. */
    fun resolveJuxc(override: String? = null): String = resolve("juxc", override)

    /** Resolve the `juxc-lsp` language-server executable. */
    fun resolveJuxcLsp(override: String? = null): String = resolve("juxc-lsp", override)

    /** Resolve the `jux` project tool. */
    fun resolveJux(override: String? = null): String = resolve("jux", override)

    /** True when a real `juxc` executable can be found by any rule above. */
    fun isConfigured(): Boolean = discover("juxc") != null

    /** The resolved absolute path of a tool, or `null` if none was found. */
    fun find(base: String): String? = discover(base)

    /**
     * Preview resolution as the settings page would see it: resolve `base`
     * treating `home` (an executable path or install root) as the override,
     * falling back to the environment when `home` is blank. Used by
     * `JuxConfigurable` to show the user what their input resolves to before
     * they apply it.
     */
    fun findPreview(base: String, home: String?): String? =
        discover(base, home?.takeIf { it.isNotBlank() })

    /**
     * Auto-detect the toolchain home from the environment (`$JUX_HOME`, `PATH`,
     * common locations) — used by the settings page's "Auto-Detect" button.
     * Returns the directory containing `juxc`, or `null`.
     */
    fun autoDetectHome(): String? = discover("juxc")?.let { File(it).parentFile?.absolutePath }

    private fun resolve(base: String, override: String?): String =
        discover(base, override) ?: (override?.takeIf { it.isNotBlank() } ?: base)

    /** Run the full search; return the first existing executable, or `null`. */
    private fun discover(base: String, override: String? = null): String? {
        val exe = exeName(base)
        // 1. Explicit override file.
        if (!override.isNullOrBlank() && override != base) {
            asFile(override)?.let { return it }
            // An override naming a *directory* (install root) is honoured too.
            inDir(File(override), exe)?.let { return it }
        }
        // 2. Configured toolchain home (direct file or install root).
        val configured = try {
            JuxSettings.getInstance().toolchainHome
        } catch (_: Throwable) {
            ""
        }
        if (configured.isNotBlank()) {
            asFile(configured)?.let { return it }
            inDir(File(configured), exe)?.let { return it }
        }
        // 3. $JUX_HOME.
        env(HOME_ENV)?.let { home -> inDir(File(home), exe)?.let { return it } }
        // 4. Each PATH entry.
        for (dir in pathDirs()) {
            inDirFlat(dir, exe)?.let { return it }
        }
        // 5. Common install locations.
        for (dir in commonDirs()) {
            inDir(dir, exe)?.let { return it }
        }
        return null
    }

    /** Look for `exe` under `root/bin/`, then directly under `root/`. */
    private fun inDir(root: File, exe: String): String? =
        inDirFlat(File(root, "bin"), exe) ?: inDirFlat(root, exe)

    /** Look for `exe` directly under `dir`. */
    private fun inDirFlat(dir: File, exe: String): String? = asFile(File(dir, exe).path)

    private fun asFile(path: String): String? = try {
        val f = File(path)
        if (f.isFile) f.absolutePath else null
    } catch (_: Exception) {
        null
    }

    private fun pathDirs(): List<File> = try {
        env("PATH")?.split(File.pathSeparatorChar)?.filter { it.isNotBlank() }?.map { File(it) }
            ?: emptyList()
    } catch (_: Exception) {
        emptyList()
    }

    private fun commonDirs(): List<File> {
        val out = ArrayList<File>()
        val home = try {
            System.getProperty("user.home").orEmpty()
        } catch (_: Exception) {
            ""
        }
        if (home.isNotEmpty()) {
            out.add(File(home, ".jux"))
            out.add(File(home, ".cargo")) // ~/.cargo/bin via inDir's bin/ probe
            out.add(File(File(home, ".cargo"), "bin"))
        }
        env("LOCALAPPDATA")?.let { out.add(File(it, "Jux")) }
        out.add(File("/usr/local"))
        out.add(File("/usr"))
        out.add(File("/opt/jux"))
        return out
    }

    private fun env(name: String): String? = try {
        System.getenv(name)?.trim()?.takeIf { it.isNotEmpty() }
    } catch (_: Exception) {
        null
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
