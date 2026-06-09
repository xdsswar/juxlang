package dev.jux.intellij.toolwindow

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.filters.TextConsoleBuilderFactory
import com.intellij.execution.process.OSProcessHandler
import com.intellij.execution.process.ProcessTerminatedListener
import com.intellij.execution.ui.ConsoleView
import com.intellij.icons.AllIcons
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.ActionToolbar
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.content.ContentFactory
import dev.jux.intellij.run.JuxToolchain
import java.io.File
import javax.swing.Icon
import javax.swing.JComponent
import javax.swing.JPanel
import java.awt.BorderLayout

/**
 * The **Jux** tool window (anchored right, like the Gradle / Cargo panels): a
 * toolbar of one-click tasks — Build, Run, Test, Build Release, Check — over a
 * live console. Each task shells out to the resolved `juxc` against the project
 * root and streams its output into the console.
 */
class JuxToolWindowFactory : ToolWindowFactory, com.intellij.openapi.project.DumbAware {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = JuxTasksPanel(project)
        val content = ContentFactory.getInstance().createContent(panel.root, "", false)
        toolWindow.contentManager.addContent(content)
    }
}

private class JuxTasksPanel(private val project: Project) {
    private val console: ConsoleView =
        TextConsoleBuilderFactory.getInstance().createBuilder(project).console
    val root: JComponent = JPanel(BorderLayout())

    init {
        val group = DefaultActionGroup().apply {
            add(task("Build", "Compile the project", AllIcons.Actions.Compile, listOf("--build")))
            add(task("Run", "Build and run the project", AllIcons.Actions.Execute, listOf("--run")))
            add(task("Test", "Run the project's tests", AllIcons.RunConfigurations.TestState.Run, listOf("--test")))
            add(task("Build Release", "Optimized release build", AllIcons.Actions.RealIntentionBulb, listOf("--build", "--release")))
            add(task("Check", "Type-check without building", AllIcons.Actions.CheckOut, emptyList()))
            addSeparator()
            add(object : AnAction("Clear", "Clear the console", AllIcons.Actions.GC) {
                override fun getActionUpdateThread() = ActionUpdateThread.EDT
                override fun actionPerformed(e: AnActionEvent) = console.clear()
            })
        }
        val toolbar: ActionToolbar =
            ActionManager.getInstance().createActionToolbar("JuxTasks", group, false)
        toolbar.targetComponent = root
        root.add(toolbar.component, BorderLayout.WEST)
        root.add(console.component, BorderLayout.CENTER)
    }

    /** A toolbar action that runs `juxc <args> <project-root>`. */
    private fun task(text: String, desc: String, icon: Icon, args: List<String>): AnAction =
        object : AnAction(text, desc, icon) {
            override fun getActionUpdateThread() = ActionUpdateThread.EDT
            override fun update(e: AnActionEvent) {
                e.presentation.isEnabled = project.basePath != null
            }
            override fun actionPerformed(e: AnActionEvent) = runJuxc(args)
        }

    private fun runJuxc(args: List<String>) {
        val base = project.basePath ?: return
        // Prefer a `src/` source root if present, else the project root.
        val input = if (File(base, "src").isDirectory) "src" else "."
        val found = JuxToolchain.find("juxc")
        if (found == null) {
            console.print(
                "Could not locate `juxc`. Configure it in Settings | Tools | Jux Toolchain, " +
                    "or set the JUX_HOME environment variable / put juxc on your PATH.\n",
                com.intellij.execution.ui.ConsoleViewContentType.ERROR_OUTPUT,
            )
            return
        }
        val juxc = found
        val cmd = GeneralCommandLine(juxc)
            .withParameters(args + input)
            .withWorkDirectory(base)
            .withCharset(Charsets.UTF_8)
        console.clear()
        console.print("> juxc ${(args + input).joinToString(" ")}\n", com.intellij.execution.ui.ConsoleViewContentType.SYSTEM_OUTPUT)
        ApplicationManager.getApplication().executeOnPooledThread {
            try {
                val handler = OSProcessHandler(cmd)
                ProcessTerminatedListener.attach(handler)
                console.attachToProcess(handler)
                handler.startNotify()
            } catch (t: Throwable) {
                console.print("Failed to start juxc: ${t.message}\n", com.intellij.execution.ui.ConsoleViewContentType.ERROR_OUTPUT)
            }
        }
    }
}
