package dev.jux.intellij.toolwindow

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.filters.TextConsoleBuilderFactory
import com.intellij.execution.process.OSProcessHandler
import com.intellij.execution.process.ProcessTerminatedListener
import com.intellij.execution.ui.ConsoleView
import com.intellij.execution.ui.ConsoleViewContentType
import com.intellij.openapi.Disposable
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.service
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.wm.ToolWindowManager
import dev.jux.intellij.run.JuxToolchain
import java.io.File

/**
 * Project-wide owner of the single Jux build/run **console** shared by both Jux
 * tool windows. The bottom **"Jux Build"** window displays [console] as its
 * content; the right **"Jux Project"** window's per-module / per-target actions
 * call [run], which streams into the very same console and surfaces the bottom
 * window so the output is visible (exactly how Cargo actions land in the Run
 * tool window in RustRover).
 *
 * Centralising the console here means there is one place that resolves a tool
 * via [JuxToolchain], one place that spawns the process off the EDT, and one
 * console the user always looks at — no matter which window triggered the build.
 */
@Service(Service.Level.PROJECT)
class JuxConsoleService(private val project: Project) : Disposable {
    /**
     * The shared console, created lazily on first use. Creation must happen on
     * the EDT (it builds Swing components); every caller below is an EDT action
     * or the bottom window's content factory, so the lazy init is always EDT.
     */
    val console: ConsoleView by lazy {
        TextConsoleBuilderFactory.getInstance().createBuilder(project).console.also {
            Disposer.register(this, it)
        }
    }

    /**
     * Run `<tool> <args>` from [workDir], streaming stdout/stderr into the shared
     * [console] and bringing the bottom "Jux Build" window to the front. [tool]
     * is a bare name (`jux` / `juxc`) resolved through [JuxToolchain]; a missing
     * tool prints a configuration hint instead of failing silently. An optional
     * [notice] (e.g. a CLI-limitation caveat) is printed just under the command
     * line. [onFinish] (if given) runs on the EDT once the process terminates —
     * used to refresh the project tree after `jux update`. Never throws; the
     * process is started on a pooled thread.
     */
    fun run(tool: String, args: List<String>, workDir: File, notice: String? = null, onFinish: (() -> Unit)? = null) {
        val c = console
        activateToolWindow()
        c.clear()
        val found = JuxToolchain.find(tool)
        if (found == null) {
            c.print(
                "Could not locate `$tool`. Configure it in Settings | Tools | Jux Toolchain, " +
                    "or set the JUX_HOME environment variable / put $tool on your PATH.\n",
                ConsoleViewContentType.ERROR_OUTPUT,
            )
            return
        }
        c.print(
            "> $tool ${args.joinToString(" ")}    (in ${workDir.name})\n",
            ConsoleViewContentType.SYSTEM_OUTPUT,
        )
        notice?.let { c.print("  $it\n", ConsoleViewContentType.LOG_WARNING_OUTPUT) }
        val cmd = GeneralCommandLine(found)
            .withParameters(args)
            .withWorkDirectory(workDir)
            .withCharset(Charsets.UTF_8)
        ApplicationManager.getApplication().executeOnPooledThread {
            // The project (and the console, disposed with it) may close between
            // scheduling and running — don't touch a disposed console.
            if (project.isDisposed) return@executeOnPooledThread
            try {
                val handler = OSProcessHandler(cmd)
                // Supersede the prior run atomically: each new handler destroys the
                // one it replaces, so no child process is ever orphaned — even when
                // two runs race and publish their handlers out of order.
                current.getAndSet(handler)?.takeIf { !it.isProcessTerminated }?.destroyProcess()
                ProcessTerminatedListener.attach(handler)
                c.attachToProcess(handler)
                if (onFinish != null) {
                    handler.addProcessListener(object : com.intellij.execution.process.ProcessListener {
                        override fun processTerminated(event: com.intellij.execution.process.ProcessEvent) {
                            ApplicationManager.getApplication().invokeLater {
                                if (!project.isDisposed) onFinish()
                            }
                        }
                    })
                }
                handler.startNotify()
            } catch (t: Throwable) {
                c.print("Failed to start $tool: ${t.message}\n", ConsoleViewContentType.ERROR_OUTPUT)
            }
        }
    }

    /** The most recently started process, so a new run can supersede it. */
    private val current = java.util.concurrent.atomic.AtomicReference<OSProcessHandler?>()

    /** Bring the bottom build window forward (no-op if it isn't registered yet). */
    private fun activateToolWindow() {
        ToolWindowManager.getInstance(project).getToolWindow(BUILD_TOOL_WINDOW_ID)?.activate(null)
    }

    override fun dispose() { /* console disposed via Disposer registration */ }

    companion object {
        /** Tool-window id of the bottom build console (see plugin.xml). */
        const val BUILD_TOOL_WINDOW_ID = "Jux Build"

        fun getInstance(project: Project): JuxConsoleService = project.service()
    }
}
