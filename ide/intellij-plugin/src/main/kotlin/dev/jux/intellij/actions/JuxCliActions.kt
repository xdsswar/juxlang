package dev.jux.intellij.actions

import com.intellij.execution.ProgramRunnerUtil
import com.intellij.execution.RunManager
import com.intellij.execution.configurations.ConfigurationTypeUtil
import com.intellij.execution.executors.DefaultRunExecutor
import com.intellij.icons.AllIcons
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.ComboBox
import com.intellij.openapi.ui.DialogWrapper
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import dev.jux.intellij.run.JuxRunConfiguration
import dev.jux.intellij.run.JuxRunConfigurationType
import dev.jux.intellij.toolwindow.JuxConsoleService
import dev.jux.intellij.toolwindow.JuxToml
import java.io.File
import javax.swing.Icon
import javax.swing.JComponent

/**
 * The argument lists of the `jux` project commands the IDE runs
 * (JUX-BUILD-SYSTEM-ADDENDUM §B.10.5, §B.14, §B.15). Pure, so every option a
 * dialog can produce unit-tests as the exact command line.
 */
object JuxCliArgs {

    /** What `jux new` scaffolds (§B.15.1). */
    enum class NewKind(val label: String, val flag: String?) {
        BINARY("Binary (a program with main)", null),
        LIBRARY("Library (--lib)", "--lib"),
        WORKSPACE("Workspace (--workspace)", "--workspace"),
    }

    fun new(name: String, kind: NewKind): List<String> = listOfNotNull("new", kind.flag, name.trim())

    /** Where an added dependency comes from. */
    enum class Source { REGISTRY, PATH, GIT }

    /** Which git reference pins a git dependency. */
    enum class GitRef(val flag: String) { BRANCH("--branch"), TAG("--tag"), REV("--rev") }

    /**
     * `jux add <name>[@version] [--path p | --git url [--branch|--tag|--rev r]]
     * [--features a,b]`. A blank version, ref or feature list leaves its
     * option out; `--path` and `--git` never appear together (the CLI
     * refuses the pair).
     */
    fun add(
        name: String,
        version: String = "",
        source: Source = Source.REGISTRY,
        location: String = "",
        gitRef: GitRef? = null,
        refValue: String = "",
        features: String = "",
    ): List<String> = buildList {
        add("add")
        val v = version.trim()
        add(if (v.isNotEmpty() && source == Source.REGISTRY) "${name.trim()}@$v" else name.trim())
        when (source) {
            Source.REGISTRY -> {}
            Source.PATH -> location.trim().takeIf { it.isNotEmpty() }?.let { add("--path"); add(it) }
            Source.GIT -> {
                location.trim().takeIf { it.isNotEmpty() }?.let { add("--git"); add(it) }
                val r = refValue.trim()
                if (gitRef != null && r.isNotEmpty()) {
                    add(gitRef.flag)
                    add(r)
                }
            }
        }
        val f = features.split(',').map { it.trim() }.filter { it.isNotEmpty() }
        if (f.isNotEmpty()) {
            add("--features")
            add(f.joinToString(","))
        }
    }

    fun remove(name: String): List<String> = listOf("remove", name.trim())

    /** `jux doc [--open]`. */
    fun doc(open: Boolean): List<String> = if (open) listOf("doc", "--open") else listOf("doc")
}

/**
 * Base for the `Tools | Jux` project commands: finds the project the action
 * is about (the nearest `jux.toml` at or above the selection, else the
 * project root) and streams `jux <args>` into the shared "Jux Build" console
 * ([JuxConsoleService]).
 */
abstract class JuxCliAction(text: String, description: String, icon: Icon?) : AnAction(text, description, icon) {

    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    /** False for commands that make a project rather than act on one. */
    protected open val needsManifest: Boolean = true

    override fun update(e: AnActionEvent) {
        val project = e.project
        e.presentation.isEnabledAndVisible = project != null && (!needsManifest || manifestDir(e) != null)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val dir = if (needsManifest) manifestDir(e) ?: return else workDir(e) ?: return
        val args = arguments(project, dir) ?: return
        JuxConsoleService.getInstance(project).run("jux", args, File(dir.path))
    }

    /** The command to run from [dir], or null when the user cancelled. */
    protected abstract fun arguments(project: Project, dir: VirtualFile): List<String>?

    /** The directory of the nearest `jux.toml` at or above the action's target. */
    protected fun manifestDir(e: AnActionEvent): VirtualFile? {
        var dir = JuxProjectContext.targetDirectory(e) ?: e.project?.basePath?.let {
            com.intellij.openapi.vfs.LocalFileSystem.getInstance().findFileByPath(it)
        }
        var hops = 0
        while (dir != null && hops < 64) {
            if (dir.findChild("jux.toml") != null) return dir
            dir = dir.parent
            hops++
        }
        return null
    }

    /** The selected directory, else the project root. */
    protected fun workDir(e: AnActionEvent): VirtualFile? =
        JuxProjectContext.targetDirectory(e) ?: e.project?.basePath?.let {
            com.intellij.openapi.vfs.LocalFileSystem.getInstance().findFileByPath(it)
        }
}

/** `jux new [--lib | --workspace] <name>`, in the selected directory. */
class JuxNewProjectCommandAction : JuxCliAction(
    "New Jux Project Here...",
    "Create a Jux binary, library or workspace with jux new",
    AllIcons.General.Add,
) {
    override val needsManifest = false

    override fun arguments(project: Project, dir: VirtualFile): List<String>? {
        val dialog = NewDialog(project)
        if (!dialog.showAndGet()) return null
        val name = dialog.name.text.trim().ifEmpty { return null }
        val kind = dialog.kind.selectedItem as? JuxCliArgs.NewKind ?: JuxCliArgs.NewKind.BINARY
        return JuxCliArgs.new(name, kind)
    }

    private class NewDialog(project: Project) : DialogWrapper(project) {
        val name = JBTextField()
        val kind = ComboBox(JuxCliArgs.NewKind.entries.toTypedArray()).apply {
            renderer = com.intellij.ui.SimpleListCellRenderer.create("") { it.label }
        }

        init {
            title = "New Jux Project"
            init()
        }

        override fun getPreferredFocusedComponent(): JComponent = name

        override fun createCenterPanel(): JComponent = FormBuilder.createFormBuilder()
            .addLabeledComponent("Name:", name)
            .addLabeledComponent("Kind:", kind)
            .panel
    }
}

/** `jux init`: a `jux.toml` for the selected directory. */
class JuxInitAction : JuxCliAction("Init Jux Project", "Add a jux.toml to this directory (jux init)", AllIcons.FileTypes.Config) {
    override val needsManifest = false

    override fun update(e: AnActionEvent) {
        val dir = workDir(e)
        e.presentation.isEnabledAndVisible = e.project != null && dir != null && dir.findChild("jux.toml") == null
    }

    override fun arguments(project: Project, dir: VirtualFile): List<String> = listOf("init")
}

/** `jux add`, from a dialog: name, version, source, git ref and features. */
class JuxAddDependencyAction : JuxCliAction(
    "Add Dependency...",
    "Add a dependency to jux.toml (jux add)",
    AllIcons.General.Add,
) {
    override fun arguments(project: Project, dir: VirtualFile): List<String>? {
        val dialog = AddDialog(project)
        if (!dialog.showAndGet()) return null
        val name = dialog.name.text.trim().ifEmpty { return null }
        return JuxCliArgs.add(
            name = name,
            version = dialog.version.text,
            source = dialog.source.selectedItem as? JuxCliArgs.Source ?: JuxCliArgs.Source.REGISTRY,
            location = dialog.location.text,
            gitRef = dialog.gitRef.selectedItem as? JuxCliArgs.GitRef,
            refValue = dialog.refValue.text,
            features = dialog.features.text,
        )
    }

    private class AddDialog(project: Project) : DialogWrapper(project) {
        val name = JBTextField().apply { emptyText.text = "com.x.json, or rust.rand for a crate" }
        val version = JBTextField().apply { emptyText.text = "1.0 (registry only)" }
        val source = ComboBox(JuxCliArgs.Source.entries.toTypedArray())
        val location = JBTextField().apply { emptyText.text = "a directory (--path) or repository URL (--git)" }
        val gitRef = ComboBox(JuxCliArgs.GitRef.entries.toTypedArray())
        val refValue = JBTextField().apply { emptyText.text = "branch, tag or revision" }
        val features = JBTextField().apply { emptyText.text = "comma-separated" }

        init {
            title = "Add Jux Dependency"
            val sync = {
                val s = source.selectedItem
                location.isEnabled = s != JuxCliArgs.Source.REGISTRY
                version.isEnabled = s == JuxCliArgs.Source.REGISTRY
                gitRef.isEnabled = s == JuxCliArgs.Source.GIT
                refValue.isEnabled = s == JuxCliArgs.Source.GIT
            }
            source.addActionListener { sync() }
            sync()
            init()
        }

        override fun getPreferredFocusedComponent(): JComponent = name

        override fun createCenterPanel(): JComponent = FormBuilder.createFormBuilder()
            .addLabeledComponent("Name:", name)
            .addLabeledComponent("Version:", version)
            .addLabeledComponent("Source:", source)
            .addLabeledComponent("Path or URL:", location)
            .addLabeledComponent("Git ref:", gitRef)
            .addLabeledComponent("Ref value:", refValue)
            .addLabeledComponent("Features:", features)
            .panel
    }
}

/** `jux remove <name>`, picked from the manifest's dependencies. */
class JuxRemoveDependencyAction : JuxCliAction(
    "Remove Dependency...",
    "Remove a dependency from jux.toml (jux remove)",
    AllIcons.General.Remove,
) {
    override fun arguments(project: Project, dir: VirtualFile): List<String>? {
        val manifest = dir.findChild("jux.toml") ?: return null
        val deps = JuxToml.dependencies(com.intellij.openapi.vfs.VfsUtilCore.loadText(manifest))
        if (deps.isEmpty()) {
            Messages.showInfoMessage(project, "This jux.toml has no dependencies.", "Remove Dependency")
            return null
        }
        val picked = Messages.showEditableChooseDialog(
            "Dependency to remove:",
            "Remove Dependency",
            null,
            deps.toTypedArray(),
            deps.first(),
            null,
        ) ?: return null
        return JuxCliArgs.remove(picked)
    }
}

/** `jux clean`: remove `target/` (and each workspace member's). */
class JuxCleanAction : JuxCliAction("Clean", "Delete the build output (jux clean)", AllIcons.Actions.GC) {
    override fun arguments(project: Project, dir: VirtualFile): List<String> = listOf("clean")
}

/** `jux tree`: the dependency tree, in the Jux Build console. */
class JuxTreeAction : JuxCliAction("Dependency Tree", "Print the dependency tree (jux tree)", AllIcons.Actions.ShowAsTree) {
    override fun arguments(project: Project, dir: VirtualFile): List<String> = listOf("tree")
}

/** `jux doc`: HTML documentation into `target/doc/`. */
class JuxDocAction : JuxCliAction("Generate Documentation", "Write the docs to target/doc (jux doc)", AllIcons.Toolwindows.Documentation) {
    override fun arguments(project: Project, dir: VirtualFile): List<String> = JuxCliArgs.doc(open = false)
}

/** `jux doc --open`: generate, then open the index in the browser. */
class JuxDocOpenAction : JuxCliAction(
    "Generate and Open Documentation",
    "Write the docs and open them in the browser (jux doc --open)",
    AllIcons.General.Web,
) {
    override fun arguments(project: Project, dir: VirtualFile): List<String> = JuxCliArgs.doc(open = true)
}

/**
 * `jux test --doc`: the ```` ```jux ```` examples of the doc comments, in the
 * test console (a doc-examples run configuration), so each example is a
 * green or red node that opens the item it documents.
 */
class JuxRunDocExamplesAction : AnAction(
    "Run Doc Examples",
    "Compile and run the ```jux examples in doc comments (jux test --doc)",
    AllIcons.RunConfigurations.TestState.Run,
) {
    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabledAndVisible = e.project != null && manifest(e) != null
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val manifest = manifest(e) ?: return
        val runManager = RunManager.getInstance(project)
        val type = ConfigurationTypeUtil.findConfigurationType(JuxRunConfigurationType::class.java)
        val settings = runManager.createConfiguration("Doc examples", type.configurationFactories.first())
        (settings.configuration as? JuxRunConfiguration)?.apply {
            mode = JuxRunConfiguration.MODE_DOCTEST
            filePath = manifest.path
        }
        settings.isTemporary = true
        runManager.addConfiguration(settings)
        runManager.selectedConfiguration = settings
        ProgramRunnerUtil.executeConfiguration(settings, DefaultRunExecutor.getRunExecutorInstance())
    }

    private fun manifest(e: AnActionEvent): VirtualFile? {
        var dir = JuxProjectContext.targetDirectory(e) ?: e.project?.basePath?.let {
            com.intellij.openapi.vfs.LocalFileSystem.getInstance().findFileByPath(it)
        }
        var hops = 0
        while (dir != null && hops < 64) {
            dir.findChild("jux.toml")?.let { return it }
            dir = dir.parent
            hops++
        }
        return null
    }
}
