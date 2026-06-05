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
 * A run configuration that executes `juxc <file> --run` (`--run` implies build
 * + execute and forwards the program's stdout/stderr and exit code).
 *
 * The `juxc` executable is resolved through [JuxToolchain] (explicit override
 * → `$JUX_HOME` → `PATH`), so the common setup needs no per-config tweaking.
 */
class JuxRunConfiguration(project: Project, factory: ConfigurationFactory, name: String) :
    RunConfigurationBase<JuxRunConfigurationOptions>(project, factory, name) {

    public override fun getOptions(): JuxRunConfigurationOptions =
        super.getOptions() as JuxRunConfigurationOptions

    /** Absolute path to the `.jux` file to run. */
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

    override fun getConfigurationEditor(): SettingsEditor<out RunConfiguration> = JuxSettingsEditor()

    /**
     * Validate before run. Throwing [RuntimeConfigurationError] is the
     * supported way to report a bad config — the IDE shows it as a dialog
     * message, never as a crash.
     */
    @Throws(RuntimeConfigurationError::class)
    override fun checkConfiguration() {
        if (filePath.isBlank()) {
            throw RuntimeConfigurationError("No Jux file specified")
        }
        val f = File(filePath)
        if (!f.isFile) {
            throw RuntimeConfigurationError("Jux file does not exist: $filePath")
        }
    }

    override fun getState(executor: Executor, environment: ExecutionEnvironment): CommandLineState {
        return object : CommandLineState(environment) {
            @Throws(ExecutionException::class)
            override fun startProcess(): ProcessHandler {
                val file = File(filePath)
                val exe = JuxToolchain.resolveJuxc(juxcPath)
                val cmd = GeneralCommandLine()
                    .withExePath(exe)
                    .withParameters(file.absolutePath, "--run")
                    .withCharset(StandardCharsets.UTF_8)
                // Run from the file's directory when we can; harmless otherwise.
                file.parentFile?.let { parent ->
                    if (parent.isDirectory) cmd.withWorkDirectory(parent)
                }
                val handler = OSProcessHandler(cmd)
                ProcessTerminatedListener.attach(handler)
                return handler
            }
        }
    }
}
