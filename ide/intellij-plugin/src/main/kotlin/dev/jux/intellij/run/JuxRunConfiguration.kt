package dev.jux.intellij.run

import com.intellij.execution.ExecutionException
import com.intellij.execution.Executor
import com.intellij.execution.configurations.CommandLineState
import com.intellij.execution.configurations.ConfigurationFactory
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.configurations.RunConfiguration
import com.intellij.execution.configurations.RunConfigurationBase
import com.intellij.execution.configurations.RuntimeConfigurationError
import com.intellij.execution.process.ColoredProcessHandler
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
 *  - **doctest** (§12.5): `jux test --doc`, the ```` ```jux ```` examples in
 *    doc comments, in the same test-tree console.
 *
 * A project run can pick a `--profile` (§B.9) and an `--example` (§B.1.3);
 * every console asks for the framed, colored diagnostics ([JuxRunCommands]).
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

    /** `--profile <name>` (§B.9) for project runs and tests; blank = the default. */
    var profile: String
        get() = options.profile
        set(value) {
            options.profile = value
        }

    /**
     * `--example <name>` (§B.1.3) for a project run; blank runs the main
     * program and [JuxRunCommands.ALL_EXAMPLES] builds every example.
     */
    var example: String
        get() = options.example
        set(value) {
            options.example = value
        }

    /**
     * True when this configuration runs a test console: `jux test`, or
     * `jux test --doc` for the doc-comment examples ([isDocTestMode]).
     */
    fun isTestMode(): Boolean = mode == MODE_TEST || mode == MODE_DOCTEST

    /** True for `jux test --doc`: the ```` ```jux ```` examples in doc comments (§12.5). */
    fun isDocTestMode(): Boolean = mode == MODE_DOCTEST

    override fun getConfigurationEditor(): SettingsEditor<out RunConfiguration> = JuxSettingsEditor()

    /**
     * Validate before run. Throwing [RuntimeConfigurationError] is the
     * supported way to report a bad config — the IDE shows it as a dialog
     * message, never as a crash.
     */
    @Throws(RuntimeConfigurationError::class)
    override fun checkConfiguration() {
        // Test mode, an example run and a profile all go through `jux`, which
        // needs a project: the file only locates the jux.toml.
        if (isTestMode() || example.isNotBlank() || profile.isNotBlank()) {
            if (manifestRoot() == null) {
                throw RuntimeConfigurationError(
                    "No jux.toml found above '${filePath.ifBlank { "<project>" }}': this configuration needs a Jux project",
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
                        .withParameters(JuxRunCommands.projectRun(example, profile))
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
                        .withParameters(JuxRunCommands.standalone(target.absolutePath))
                        .withCharset(StandardCharsets.UTF_8)
                    val workDir = if (target.isDirectory) target else target.parentFile
                    workDir?.let { if (it.isDirectory) c.withWorkDirectory(it) }
                    c
                }
                // Colored: the tools print the framed `human` diagnostics
                // with ANSI colors (JuxRunCommands.CONSOLE_DIAGNOSTICS).
                val handler = ColoredProcessHandler(cmd)
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
        const val MODE_DOCTEST = "doctest"
    }
}
