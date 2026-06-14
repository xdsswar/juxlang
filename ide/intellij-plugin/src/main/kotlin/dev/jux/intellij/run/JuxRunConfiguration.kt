package dev.jux.intellij.run

import com.intellij.execution.ExecutionException
import com.intellij.execution.Executor
import com.intellij.execution.configurations.CommandLineState
import com.intellij.execution.configurations.ConfigurationFactory
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.configurations.RunConfiguration
import com.intellij.execution.configurations.RunConfigurationBase
import com.intellij.execution.configurations.RuntimeConfigurationError
import com.intellij.execution.process.OSProcessHandler
import com.intellij.execution.process.ProcessHandler
import com.intellij.execution.process.ProcessTerminatedListener
import com.intellij.execution.runners.ExecutionEnvironment
import com.intellij.openapi.options.SettingsEditor
import com.intellij.openapi.project.Project
import java.io.File
import java.nio.charset.StandardCharsets

/**
 * A run configuration with two modes:
 *
 *  - **run** (default): for a Jux PROJECT (a `jux.toml` above the file) runs
 *    `jux run` from the manifest root, so `rust.<crate>` / Jux dependencies are
 *    resolved and LINKED; for a standalone file with no manifest, `juxc <file>
 *    --run`. Both build + execute and forward the program's stdout/stderr and
 *    exit code. (`juxc` is the bare compiler — it can't read `jux.toml` or link
 *    dependencies, so a project must go through `jux`.)
 *  - **test** (§TS.2): `jux test [pattern] [--release]` from the project's
 *    manifest root, with the SM test-tree console ([JuxTestCommandLineState]).
 *
 * The executables resolve through [JuxToolchain] (explicit override →
 * `$JUX_HOME` → `PATH`), so the common setup needs no per-config tweaking.
 */
class JuxRunConfiguration(project: Project, factory: ConfigurationFactory, name: String) :
    RunConfigurationBase<JuxRunConfigurationOptions>(project, factory, name) {

    public override fun getOptions(): JuxRunConfigurationOptions =
        super.getOptions() as JuxRunConfigurationOptions

    /** Absolute path to the `.jux` file to run (test mode: any file/dir in the project). */
    var filePath: String
        get() = options.filePath
        set(value) {
            options.filePath = value
        }

    /** Optional explicit juxc path; blank means "auto-resolve". */
    var juxcPath: String
        get() = options.juxcPath
        set(value) {
            options.juxcPath = value
        }

    /** `"run"` or `"test"` — selects the command line and console kind. */
    var mode: String
        get() = options.mode
        set(value) {
            options.mode = value
        }

    /** `jux test <pattern>` substring filter (§TS.8); blank runs all tests. */
    var testPattern: String
        get() = options.testPattern
        set(value) {
            options.testPattern = value
        }

    /** Build the test runner optimized (`jux test --release`). */
    var release: Boolean
        get() = options.release
        set(value) {
            options.release = value
        }

    /** True when this configuration runs `jux test` rather than `juxc --run`. */
    fun isTestMode(): Boolean = mode == MODE_TEST

    override fun getConfigurationEditor(): SettingsEditor<out RunConfiguration> = JuxSettingsEditor()

    /**
     * Validate before run. Throwing [RuntimeConfigurationError] is the
     * supported way to report a bad config — the IDE shows it as a dialog
     * message, never as a crash.
     */
    @Throws(RuntimeConfigurationError::class)
    override fun checkConfiguration() {
        if (isTestMode()) {
            // Test mode points at any file/dir used only to locate the
            // manifest; what must exist is the jux.toml `jux test` requires.
            if (manifestRoot() == null) {
                throw RuntimeConfigurationError(
                    "No jux.toml found above '${filePath.ifBlank { "<project>" }}' — `jux test` needs a Jux project",
                )
            }
            return
        }
        if (filePath.isBlank()) {
            throw RuntimeConfigurationError("No Jux file specified")
        }
        val f = File(filePath)
        if (!f.isFile) {
            throw RuntimeConfigurationError("Jux file does not exist: $filePath")
        }
    }

    override fun getState(executor: Executor, environment: ExecutionEnvironment): CommandLineState {
        if (isTestMode()) return JuxTestCommandLineState(this, environment)
        return object : CommandLineState(environment) {
            init {
                // Make `path:line:col` in juxc's output clickable (jumps to the
                // exact spot), like Java's compiler console.
                addConsoleFilters(JuxConsoleFilter(environment.project))
            }

            @Throws(ExecutionException::class)
            override fun startProcess(): ProcessHandler {
                // A Jux PROJECT (a `jux.toml` above the file) is built by the
                // `jux` project tool: it reads the manifest and resolves + LINKS
                // `rust.<crate>` / Jux dependencies. The bare `juxc` compiler
                // can't do that — `juxc --run` on a project with deps fails to
                // resolve/link them. So only a standalone, manifest-less file
                // falls back to `juxc <file> --run`.
                val manifest = manifestRoot()
                val cmd = if (manifest != null) {
                    GeneralCommandLine()
                        .withExePath(JuxToolchain.resolveJux())
                        .withParameters("run")
                        .withWorkDirectory(manifest)
                        .withCharset(StandardCharsets.UTF_8)
                } else {
                    // No manifest: compile the whole source tree the file belongs
                    // to (juxc walks a directory recursively) so cross-file
                    // `import`s resolve; passing just the one file would leave its
                    // imported types uncompiled.
                    val target = compileTarget(File(filePath))
                    val c = GeneralCommandLine()
                        .withExePath(JuxToolchain.resolveJuxc(juxcPath))
                        .withParameters(target.absolutePath, "--run")
                        .withCharset(StandardCharsets.UTF_8)
                    val workDir = if (target.isDirectory) target else target.parentFile
                    workDir?.let { if (it.isDirectory) c.withWorkDirectory(it) }
                    c
                }
                val handler = OSProcessHandler(cmd)
                ProcessTerminatedListener.attach(handler)
                return handler
            }
        }
    }

    /**
     * The compile target for `file`: the directory juxc should walk so the whole
     * project builds together and cross-file `import`s resolve.
     *
     * Prefer the **manifest root** (the nearest ancestor with a `jux.toml`): juxc
     * reads the manifest and compiles the declared package, so dependencies and
     * `[[bin]]` targets resolve too. Jux does not require the directory layout to
     * mirror the package path, so the package-strip below is only a fallback for
     * manifest-less files: strip the file's `package a.b.c;` depth off its
     * directory to find the source root. A file with no package compiles alone.
     */
    private fun compileTarget(file: File): File {
        manifestRoot()?.let { return it }
        return try {
            val pkg = PACKAGE_RE.find(file.readText())?.groupValues?.get(1)?.trim().orEmpty()
            if (pkg.isEmpty()) return file
            val segments = pkg.split('.').count { it.isNotBlank() }
            var dir: File? = file.parentFile
            repeat(segments) { dir = dir?.parentFile }
            dir?.takeIf { it.isDirectory } ?: file
        } catch (_: Exception) {
            file
        }
    }

    companion object {
        private val PACKAGE_RE = Regex("""(?m)^\s*package\s+([A-Za-z_][\w.]*)\s*;""")

        /** [mode] values. */
        const val MODE_RUN = "run"
        const val MODE_TEST = "test"
    }
}
