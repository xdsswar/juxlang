package dev.jux.intellij.run

import com.intellij.execution.DefaultExecutionResult
import com.intellij.execution.ExecutionException
import com.intellij.execution.ExecutionResult
import com.intellij.execution.Executor
import com.intellij.execution.configurations.CommandLineState
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.process.OSProcessHandler
import com.intellij.execution.process.ProcessHandler
import com.intellij.execution.process.ProcessTerminatedListener
import com.intellij.execution.runners.ExecutionEnvironment
import com.intellij.execution.runners.ProgramRunner
import com.intellij.execution.testframework.sm.SMTestRunnerConnectionUtil
import java.io.File
import java.nio.charset.StandardCharsets

/**
 * Process + console state for a test-mode [JuxRunConfiguration]: runs
 * `jux test [pattern] [--release]` from the project's manifest root (`jux test`
 * requires `jux.toml` in its working directory, §TS.2) and attaches the SM
 * test-tree console ([JuxTestConsoleProperties]) instead of a plain one.
 */
class JuxTestCommandLineState(
    private val config: JuxRunConfiguration,
    environment: ExecutionEnvironment,
) : CommandLineState(environment) {

    @Throws(ExecutionException::class)
    override fun startProcess(): ProcessHandler {
        val workDir = config.manifestRoot()
            ?: throw ExecutionException("No jux.toml found above '${config.filePath}' — `jux test` needs a Jux project")
        val exe = JuxToolchain.resolveJux()
        val params = buildList {
            add("test")
            config.testPattern.takeIf { it.isNotBlank() }?.let { add(it) }
            if (config.release) add("--release")
        }
        val cmd = GeneralCommandLine()
            .withExePath(exe)
            .withParameters(params)
            .withWorkDirectory(workDir)
            .withCharset(StandardCharsets.UTF_8)
        val handler = OSProcessHandler(cmd)
        ProcessTerminatedListener.attach(handler)
        return handler
    }

    @Throws(ExecutionException::class)
    override fun execute(executor: Executor, runner: ProgramRunner<*>): ExecutionResult {
        val handler = startProcess()
        val console = SMTestRunnerConnectionUtil.createAndAttachConsole(
            JuxTestConsoleProperties.FRAMEWORK_NAME,
            handler,
            JuxTestConsoleProperties(config, executor),
        )
        // Clickable `path:line:col` links in compile output the test run prints.
        console.addMessageFilter(JuxConsoleFilter(config.project))
        return DefaultExecutionResult(console, handler, *createActions(console, handler, executor))
    }
}

/**
 * The nearest ancestor directory of the configuration's file/directory that
 * contains a `jux.toml` manifest, falling back to the project root when it has
 * one. Null when no manifest exists anywhere — test mode cannot run then.
 */
internal fun JuxRunConfiguration.manifestRoot(): File? {
    var dir: File? = File(filePath).let { if (it.isDirectory) it else it.parentFile }
    while (dir != null) {
        if (File(dir, "jux.toml").isFile) return dir
        dir = dir.parentFile
    }
    val base = project.basePath?.let(::File) ?: return null
    return if (File(base, "jux.toml").isFile) base else null
}
