package dev.jux.intellij.toolwindow

import com.google.gson.JsonObject
import com.google.gson.JsonParser
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.process.CapturingProcessHandler
import com.intellij.icons.AllIcons
import com.intellij.ide.actions.RevealFileAction
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.ActionToolbar
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.InputValidator
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.PopupHandler
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.content.ContentFactory
import com.intellij.ui.treeStructure.Tree
import dev.jux.intellij.project.JuxProjectKind
import dev.jux.intellij.project.JuxScaffold
import dev.jux.intellij.run.JuxToolchain
import dev.jux.intellij.settings.JuxSettings
import java.awt.BorderLayout
import java.io.File
import javax.swing.Icon
import javax.swing.JPanel
import javax.swing.tree.DefaultMutableTreeNode
import javax.swing.tree.DefaultTreeModel
import javax.swing.tree.TreePath

/**
 * The **Jux Project** tool window (anchored right, modelled on RustRover's Cargo
 * panel): a tree of the workspace's modules, each expanding to a **targets**
 * group (its `[[bin]]` / `[lib]` artifacts), its edition and dependencies. The
 * toolbar and a right-click menu **build / run / test / check** whichever module
 * or target is selected, streaming into the shared [JuxConsoleService] console.
 *
 * The tree is **authoritative**: it runs `jux metadata --format json` (the CLI's
 * machine-readable project model) and renders real resolved targets, dependency
 * sources, profiles, and artifact paths. Per-node actions select precisely via
 * `-p <package>` / `--bin <name>` / `--lib`, and the cross-compile triple (set by
 * the "Target" action, listed from `jux target list`) flows to build/run/check.
 * If the toolchain is too old to answer `jux metadata`, the panel falls back to a
 * minimal hand-parse of `jux.toml` ([JuxToml]).
 */
class JuxProjectToolWindowFactory : ToolWindowFactory, DumbAware {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = JuxProjectPanel(project)
        val content = ContentFactory.getInstance().createContent(panel.root, "", false)
        toolWindow.contentManager.addContent(content)
    }
}

/** What a selected tree node lets you build/run, and how to select it on the CLI. */
private enum class Ctx { WORKSPACE, MODULE, BIN, LIB }

/**
 * The build context carried by an actionable node. Builds always run from the
 * workspace root and select the unit by flag: [pkg] → `-p`, [bin] → `--bin`,
 * [lib] → `--lib`. [runnable] is false for libraries.
 */
private data class BuildContext(
    val kind: Ctx,
    val pkg: String? = null,
    val bin: String? = null,
    val lib: Boolean = false,
    val runnable: Boolean = true,
)

/**
 * A tree node payload: primary [label], optional grayed [tail], [icon], an
 * optional [file] to open on double-click, an optional [build] context that makes
 * the node a build/run target, and any [artifacts] (expected output paths) the
 * "Reveal Artifact" action can jump to.
 */
private data class JuxNode(
    val label: String,
    val icon: Icon,
    val tail: String? = null,
    val file: File? = null,
    val build: BuildContext? = null,
    val artifacts: List<File> = emptyList(),
)

// ---- Parsed `jux metadata --format json` model ----------------------------

private data class JuxMetadata(
    val workspaceRoot: File,
    val isWorkspace: Boolean,
    val packages: List<JuxPackage>,
)

private data class JuxPackage(
    val name: String,
    val manifest: File,
    val root: File,
    val version: String?,
    val edition: String?,
    val isLib: Boolean,
    val targets: List<JuxTarget>,
    val deps: List<Pair<String, String>>,
)

private data class JuxTarget(
    val kind: String, // "lib" | "bin"
    val name: String,
    val tail: String?, // entry / crate-type
    val artifacts: List<File>,
)

private class JuxProjectPanel(private val project: Project) {
    val root: JPanel = JPanel(BorderLayout())
    private val treeRoot = DefaultMutableTreeNode(JuxNode("Jux", AllIcons.Nodes.ModuleGroup))
    private val model = DefaultTreeModel(treeRoot)
    private val tree = Tree(model)

    /** Last computed workspace root — the working directory for every action. */
    private var workspaceRoot: File? = null

    init {
        tree.isRootVisible = true
        tree.showsRootHandles = true
        tree.cellRenderer = JuxModuleCellRenderer()
        tree.addMouseListener(object : java.awt.event.MouseAdapter() {
            override fun mousePressed(e: java.awt.event.MouseEvent) {
                if (e.isPopupTrigger) selectRowAt(e)
            }

            override fun mouseReleased(e: java.awt.event.MouseEvent) {
                if (e.isPopupTrigger) selectRowAt(e)
            }

            override fun mouseClicked(e: java.awt.event.MouseEvent) {
                if (e.clickCount == 2) onDoubleClick()
            }
        })

        val toolbar: ActionToolbar =
            ActionManager.getInstance().createActionToolbar("JuxProject", toolbarGroup(), true)
        toolbar.targetComponent = root
        PopupHandler.installPopupMenu(tree, popupGroup(), "JuxProjectPopup")

        root.add(toolbar.component, BorderLayout.NORTH)
        root.add(JBScrollPane(tree), BorderLayout.CENTER)
        reload()
    }

    // ---- Action groups -----------------------------------------------------

    /** Build/run actions shared by the toolbar and the context menu. */
    private fun coreActions(): List<AnAction> = listOf(
        buildAction("Build", "Compile the selection", AllIcons.Actions.Compile, "build"),
        buildAction("Run", "Build and run the selection", AllIcons.Actions.Execute, "run", runOnly = true),
        buildAction("Build Release", "Optimized release build", AllIcons.Actions.RealIntentionBulb, "build", release = true),
        buildAction("Test", "Run the selection's tests", AllIcons.RunConfigurations.TestState.Run, "test"),
        buildAction("Check", "Type-check without building", AllIcons.Actions.CheckOut, "check"),
    )

    private fun toolbarGroup(): DefaultActionGroup = DefaultActionGroup().apply {
        add(newModuleAction())
        addSeparator()
        coreActions().forEach(::add)
        addSeparator()
        add(CrossTargetAction())
        addSeparator()
        // Re-fetch git dependencies (`jux update`), then rebuild the tree.
        add(object : AnAction("Reload Dependencies", "Re-fetch git dependencies (jux update), then refresh", AllIcons.Vcs.Fetch) {
            override fun getActionUpdateThread() = ActionUpdateThread.EDT
            override fun update(e: AnActionEvent) {
                e.presentation.isEnabled = workspaceRoot != null
            }
            override fun actionPerformed(e: AnActionEvent) {
                val base = workspaceRoot ?: return
                JuxConsoleService.getInstance(project).run("jux", listOf("update"), base) { reload() }
            }
        })
        // Rebuild the tree from `jux metadata` without touching dependencies.
        add(object : AnAction("Refresh", "Reload the project model", AllIcons.Actions.Refresh) {
            override fun getActionUpdateThread() = ActionUpdateThread.EDT
            override fun actionPerformed(e: AnActionEvent) = reload()
        })
    }

    private fun popupGroup(): DefaultActionGroup = DefaultActionGroup().apply {
        coreActions().forEach(::add)
        addSeparator()
        add(revealArtifactAction())
        add(openManifestAction())
        addSeparator()
        add(newModuleAction())
    }

    /**
     * One context-aware build action. [verb] is the `jux` subcommand; [release]
     * adds `--release`; [runOnly] actions (Run) are disabled for non-runnable
     * nodes (libraries). Enablement and the command both follow the selection.
     */
    private fun buildAction(
        text: String,
        desc: String,
        icon: Icon,
        verb: String,
        release: Boolean = false,
        runOnly: Boolean = false,
    ): AnAction = object : AnAction(text, desc, icon) {
        override fun getActionUpdateThread() = ActionUpdateThread.EDT
        override fun update(e: AnActionEvent) {
            val ctx = currentContext()
            e.presentation.isEnabled = ctx != null && (!runOnly || ctx.runnable)
        }
        override fun actionPerformed(e: AnActionEvent) {
            currentContext()?.let { execute(verb, release, it) }
        }
    }

    /** "Reveal Artifact" — jump to a target's built binary / library on disk. */
    private fun revealArtifactAction(): AnAction =
        object : AnAction("Reveal Artifact", "Show the built output in the file manager", AllIcons.Actions.MenuOpen) {
            override fun getActionUpdateThread() = ActionUpdateThread.EDT
            override fun update(e: AnActionEvent) {
                e.presentation.isEnabledAndVisible = selectedNode()?.artifacts?.isNotEmpty() == true
            }
            override fun actionPerformed(e: AnActionEvent) {
                val arts = selectedNode()?.artifacts ?: return
                arts.firstOrNull { it.isFile }?.let { RevealFileAction.openFile(it); return }
                arts.firstOrNull()?.parentFile?.takeIf { it.isDirectory }?.let { RevealFileAction.openDirectory(it) }
                    ?: Messages.showInfoMessage(project, "Nothing built yet for this target.", "Reveal Artifact")
            }
        }

    /** "Open jux.toml" — open the selected module's manifest. */
    private fun openManifestAction(): AnAction =
        object : AnAction("Open jux.toml", "Open the module manifest", AllIcons.FileTypes.Config) {
            override fun getActionUpdateThread() = ActionUpdateThread.EDT
            override fun update(e: AnActionEvent) {
                e.presentation.isEnabledAndVisible = selectedNode()?.file?.isFile == true
            }
            override fun actionPerformed(e: AnActionEvent) {
                selectedNode()?.file?.let(::openFile)
            }
        }

    /** "New Module" — scaffold a library module and add it to the workspace. */
    private fun newModuleAction(): AnAction =
        object : AnAction("New Module", "Add a new library module to this workspace", AllIcons.General.Add) {
            override fun getActionUpdateThread() = ActionUpdateThread.EDT
            override fun update(e: AnActionEvent) {
                e.presentation.isEnabled = workspaceRoot != null
            }
            override fun actionPerformed(e: AnActionEvent) = promptAndCreateModule()
        }

    /** Ask for a module name, then scaffold a `[lib]` module under the root. */
    private fun promptAndCreateModule() {
        val base = workspaceRoot ?: return
        val rootVf = LocalFileSystem.getInstance().findFileByIoFile(base) ?: return
        val name = Messages.showInputDialog(
            project,
            "Name of the new library module (created under the workspace root):",
            "New Jux Module",
            AllIcons.General.Add,
            "",
            object : InputValidator {
                // A safe directory name that doesn't already exist at the root.
                override fun checkInput(input: String?): Boolean {
                    val n = input?.trim().orEmpty()
                    return n.isNotEmpty() &&
                        n.all { it.isLetterOrDigit() || it == '_' || it == '-' } &&
                        rootVf.findChild(n) == null
                }
                override fun canClose(input: String?): Boolean = checkInput(input)
            },
        )?.trim().orEmpty()
        if (name.isEmpty()) return
        createLibraryModule(rootVf, name)
    }

    /**
     * Scaffold `<root>/<name>/` as a library: a `[lib]` `jux.toml`, a
     * `src/lib.jux` API stub, and a `[workspace] members` entry in the root
     * manifest (created if absent — turning a single package into a workspace).
     * Editing the root `jux.toml` also fires the dependency re-discovery, and the
     * tree reloads to show the new module. Reuses [JuxScaffold] (the same content
     * the New Project wizard writes).
     */
    private fun createLibraryModule(rootVf: VirtualFile, name: String) {
        var ok = false
        WriteCommandAction.runWriteCommandAction(project) {
            ok = try {
                val modDir = VfsUtil.createDirectoryIfMissing(rootVf, name)
                if (modDir == null) {
                    false
                } else {
                    val pkg = JuxScaffold.packageNameFor(name)
                    writeChild(modDir, "jux.toml", JuxScaffold.manifest(pkg, JuxProjectKind.LIBRARY, "lib"))
                    VfsUtil.createDirectoryIfMissing(modDir, "src")?.let { src ->
                        writeChild(
                            src,
                            JuxScaffold.entryFileName(JuxProjectKind.LIBRARY),
                            JuxScaffold.entryContent(JuxProjectKind.LIBRARY, sample = true),
                        )
                    }
                    addWorkspaceMember(rootVf, name)
                    true
                }
            } catch (_: Throwable) {
                false
            }
        }
        if (!ok) {
            Messages.showErrorDialog(project, "Could not create module `$name`.", "New Jux Module")
            return
        }
        reload()
        rootVf.findFileByRelativePath("$name/src/${JuxScaffold.entryFileName(JuxProjectKind.LIBRARY)}")
            ?.let { FileEditorManager.getInstance(project).openFile(it, true) }
    }

    /** Write (or overwrite) a child file under [dir] with [content]. */
    private fun writeChild(dir: VirtualFile, fileName: String, content: String) {
        val f = dir.findChild(fileName) ?: dir.createChildData(this, fileName)
        VfsUtil.saveText(f, content)
    }

    /** Add [member] to the root `[workspace] members`, creating the section if needed. */
    private fun addWorkspaceMember(rootVf: VirtualFile, member: String) {
        val manifest = rootVf.findChild("jux.toml") ?: rootVf.createChildData(this, "jux.toml")
        val text = VfsUtil.loadText(manifest)
        JuxToml.withWorkspaceMember(text, member)?.let { VfsUtil.saveText(manifest, it) }
    }

    /** The "Target: <triple>" action — picks the cross-compile triple. */
    private inner class CrossTargetAction : AnAction() {
        override fun getActionUpdateThread() = ActionUpdateThread.BGT
        override fun update(e: AnActionEvent) {
            e.presentation.icon = AllIcons.General.Settings
            e.presentation.text = "Target: " + (crossTarget() ?: "host")
            e.presentation.description = "Set the cross-compile target triple for builds"
        }
        override fun actionPerformed(e: AnActionEvent) {
            val current = JuxSettings.getInstance().crossTarget
            // Offer the installed triples (from `jux target list --installed`) as an
            // editable chooser; fall back to a plain input if rustup isn't around.
            val installed = installedTargets()
            val choice = if (installed.isNotEmpty()) {
                val options = (listOf("") + installed).toTypedArray() // "" = host
                Messages.showEditableChooseDialog(
                    "Rust target triple for builds (blank = host):",
                    "Jux Build Target",
                    AllIcons.General.Settings,
                    options,
                    current.ifBlank { "" },
                    null,
                )
            } else {
                Messages.showInputDialog(
                    project,
                    "Rust target triple for builds (blank = host):\n" +
                        "e.g. x86_64-pc-windows-msvc, x86_64-unknown-linux-gnu, aarch64-apple-darwin",
                    "Jux Build Target",
                    AllIcons.General.Settings,
                    current,
                    null,
                )
            } ?: return
            JuxSettings.getInstance().crossTarget = choice.trim()
        }
    }

    // ---- Command execution -------------------------------------------------

    /**
     * Assemble and stream the `jux` command. Selection flows through `-p` /
     * `--bin` / `--lib` (each verb only gets the flags it accepts); the
     * cross-compile triple is forwarded to build/run/check. Every action runs
     * from the workspace root.
     */
    private fun execute(verb: String, release: Boolean, ctx: BuildContext) {
        val base = workspaceRoot ?: return
        val args = ArrayList<String>()
        args.add(verb)
        ctx.pkg?.let { args.add("-p"); args.add(it) }
        // Target selectors: --bin on build/run, --lib on build only.
        if (ctx.bin != null && (verb == "build" || verb == "run")) { args.add("--bin"); args.add(ctx.bin) }
        if (ctx.lib && verb == "build") args.add("--lib")
        if (release && verb != "check") args.add("--release")
        // --target is accepted by build / run / check (not test).
        if (verb != "test") crossTarget()?.let { args.add("--target"); args.add(it) }
        JuxConsoleService.getInstance(project).run("jux", args, base)
    }

    /**
     * The effective build context: the selected node's own context, or — when a
     * non-actionable node (or nothing) is selected — the whole workspace, so the
     * toolbar can always build the project.
     */
    private fun currentContext(): BuildContext? {
        selectedNode()?.build?.let { return it }
        return if (workspaceRoot != null) BuildContext(Ctx.WORKSPACE) else null
    }

    private fun selectedNode(): JuxNode? =
        (tree.lastSelectedPathComponent as? DefaultMutableTreeNode)?.userObject as? JuxNode

    /** The cross-compile triple: settings override, else the root `[build] target`. */
    private fun crossTarget(): String? {
        JuxSettings.getInstance().crossTarget.takeIf { it.isNotBlank() }?.let { return it }
        val base = workspaceRoot ?: return null
        return JuxToml.buildTarget(readFileOrEmpty(File(base, "jux.toml")))
    }

    /** Installed cross-compile triples via `jux target list --installed`. */
    private fun installedTargets(): List<String> {
        val jux = JuxToolchain.find("jux") ?: return emptyList()
        val base = workspaceRoot ?: return emptyList()
        return try {
            val cmd = GeneralCommandLine(jux, "target", "list", "--installed")
                .withWorkDirectory(base).withCharset(Charsets.UTF_8)
            val out = CapturingProcessHandler(cmd).runProcess(10_000)
            if (out.exitCode != 0) return emptyList()
            out.stdout.lineSequence()
                .map { it.removeSuffix("(installed)").removeSuffix("(default)").trim() }
                .filter { it.isNotEmpty() && !it.contains(' ') }
                .toList()
        } catch (_: Throwable) {
            emptyList()
        }
    }

    private fun selectRowAt(e: java.awt.event.MouseEvent) {
        val row = tree.getClosestRowForLocation(e.x, e.y)
        if (row >= 0) tree.setSelectionRow(row)
    }

    /** Double-click: open a node's file, else run its default action. */
    private fun onDoubleClick() {
        val node = selectedNode() ?: return
        if (node.file != null) {
            openFile(node.file)
            return
        }
        val ctx = node.build ?: return
        val verb = if (ctx.runnable && (ctx.kind == Ctx.BIN || ctx.kind == Ctx.MODULE)) "run" else "build"
        execute(verb, false, ctx)
    }

    private fun openFile(file: File) {
        val vf = LocalFileSystem.getInstance().findFileByIoFile(file) ?: return
        if (vf.isDirectory) return
        FileEditorManager.getInstance(project).openFile(vf, true)
    }

    // ---- Model construction (off the EDT) ----------------------------------

    /**
     * Rebuild the tree. The `jux metadata` call + manifest reads run on a pooled
     * thread (never the EDT — they shell out and walk disk), then the tree model
     * is swapped on the EDT.
     */
    private fun reload() {
        val base = project.basePath?.let(::File)
        ApplicationManager.getApplication().executeOnPooledThread {
            val (rootLabel, children) = computeModel(base)
            ApplicationManager.getApplication().invokeLater {
                workspaceRoot = base?.takeIf { it.isDirectory }
                treeRoot.userObject = rootLabel
                treeRoot.removeAllChildren()
                for (c in children) treeRoot.add(c)
                model.reload()
                expandModules()
            }
        }
    }

    /** Off-EDT model: prefer `jux metadata`, fall back to a `jux.toml` scan. */
    private fun computeModel(base: File?): Pair<JuxNode, List<DefaultMutableTreeNode>> {
        if (base == null || !base.isDirectory) {
            return JuxNode("No project", AllIcons.General.Information) to emptyList()
        }
        loadMetadata(base)?.let { return modelFromMetadata(base, it) }
        return modelFromToml(base)
    }

    // ---- Authoritative path: `jux metadata --format json` ------------------

    /** Run `jux metadata --format json`; parse it, or `null` on any failure. */
    private fun loadMetadata(base: File): JuxMetadata? {
        val jux = JuxToolchain.find("jux") ?: return null
        return try {
            val args = arrayListOf("metadata", "--format", "json")
            JuxSettings.getInstance().crossTarget.takeIf { it.isNotBlank() }
                ?.let { args.add("--target"); args.add(it) }
            val cmd = GeneralCommandLine(jux).withParameters(args)
                .withWorkDirectory(base).withCharset(Charsets.UTF_8)
            val out = CapturingProcessHandler(cmd).runProcess(20_000)
            if (out.exitCode != 0) return null
            parseMetadata(out.stdout)
        } catch (_: Throwable) {
            null
        }
    }

    private fun parseMetadata(json: String): JuxMetadata? {
        val o = try {
            JsonParser.parseString(json).takeIf { it.isJsonObject }?.asJsonObject ?: return null
        } catch (_: Throwable) {
            return null
        }
        val wsRoot = o.str("workspace_root")?.let(::File) ?: return null
        val packages = o.get("packages")?.takeIf { it.isJsonArray }?.asJsonArray
            ?.mapNotNull { it.takeIf { e -> e.isJsonObject }?.asJsonObject?.let(::parsePackage) }
            ?: emptyList()
        return JuxMetadata(wsRoot, o.bool("is_workspace"), packages)
    }

    private fun parsePackage(p: JsonObject): JuxPackage? {
        val name = p.str("name") ?: return null
        val rootDir = p.str("root")?.let(::File) ?: return null
        val manifest = p.str("manifest_path")?.let(::File) ?: File(rootDir, "jux.toml")
        val targets = p.get("targets")?.takeIf { it.isJsonArray }?.asJsonArray
            ?.mapNotNull { it.takeIf { e -> e.isJsonObject }?.asJsonObject?.let(::parseTarget) }
            ?: emptyList()
        val deps = p.get("dependencies")?.takeIf { it.isJsonArray }?.asJsonArray
            ?.mapNotNull { it.takeIf { e -> e.isJsonObject }?.asJsonObject?.let(::parseDep) }
            ?: emptyList()
        return JuxPackage(
            name = name,
            manifest = manifest,
            root = rootDir,
            version = p.str("version"),
            edition = p.str("edition"),
            isLib = targets.any { it.kind == "lib" },
            targets = targets,
            deps = deps,
        )
    }

    private fun parseTarget(t: JsonObject): JuxTarget? {
        val kind = t.str("kind") ?: return null
        val name = t.str("name") ?: return null
        val tail = when (kind) {
            "bin" -> t.str("entry") ?: "bin"
            "lib" -> t.get("crate_type")?.takeIf { it.isJsonArray }?.asJsonArray
                ?.mapNotNull { e -> e.takeIf { !it.isJsonNull }?.asString }?.joinToString(", ")
                ?.ifEmpty { "lib" } ?: "lib"
            else -> null
        }
        val artifact = t.get("artifact")?.takeIf { it.isJsonObject }?.asJsonObject
        val artifacts = listOfNotNull(
            artifact?.str("release")?.let(::File),
            artifact?.str("debug")?.let(::File),
        )
        return JuxTarget(kind, name, tail, artifacts)
    }

    private fun parseDep(d: JsonObject): Pair<String, String>? {
        val name = d.str("name") ?: return null
        val detail = when (d.str("source")) {
            "path" -> "path: " + (d.str("path") ?: "")
            "git" -> "git: " + (d.str("git") ?: "") + (d.str("ref")?.let { " ($it)" } ?: "")
            else -> d.str("version") ?: ""
        }
        return name to detail
    }

    private fun modelFromMetadata(base: File, meta: JuxMetadata): Pair<JuxNode, List<DefaultMutableTreeNode>> {
        val rootLabel = JuxNode(base.name, AllIcons.Nodes.ModuleGroup, build = BuildContext(Ctx.WORKSPACE))
        val isWs = meta.isWorkspace
        val children = meta.packages.map { pkg -> moduleNodeFromPackage(pkg, isWs) }
        return rootLabel to children.ifEmpty {
            listOf(DefaultMutableTreeNode(JuxNode("No packages", AllIcons.General.Information)))
        }
    }

    private fun moduleNodeFromPackage(pkg: JuxPackage, isWorkspace: Boolean): DefaultMutableTreeNode {
        // In a workspace, select members by `-p <name>`; a lone package needs none.
        val pkgSel = pkg.name.takeIf { isWorkspace }
        val hasBin = pkg.targets.any { it.kind == "bin" }
        val node = DefaultMutableTreeNode(
            JuxNode(
                pkg.name,
                if (pkg.isLib) AllIcons.Nodes.PpLibFolder else AllIcons.Nodes.Module,
                tail = pkg.version,
                file = pkg.manifest.takeIf { it.isFile },
                build = BuildContext(Ctx.MODULE, pkg = pkgSel, runnable = hasBin),
            ),
        )

        val targets = group("targets", AllIcons.Nodes.ModuleGroup)
        for (t in pkg.targets) {
            val isBin = t.kind == "bin"
            targets.add(
                DefaultMutableTreeNode(
                    JuxNode(
                        t.name,
                        if (isBin) AllIcons.Actions.Execute else AllIcons.Nodes.PpLib,
                        tail = t.tail,
                        build = if (isBin) {
                            BuildContext(Ctx.BIN, pkg = pkgSel, bin = t.name, runnable = true)
                        } else {
                            BuildContext(Ctx.LIB, pkg = pkgSel, lib = true, runnable = false)
                        },
                        artifacts = t.artifacts,
                    ),
                ),
            )
        }
        node.add(targets)

        pkg.edition?.let { node.add(leaf("Edition", AllIcons.Nodes.Tag, tail = it)) }

        if (pkg.deps.isNotEmpty()) {
            val g = group("Dependencies", AllIcons.Nodes.PpLibFolder)
            for ((depName, detail) in pkg.deps) {
                g.add(leaf(depName, AllIcons.Nodes.PpLib, tail = detail.ifEmpty { null }))
            }
            node.add(g)
        }
        return node
    }

    // ---- Fallback path: minimal `jux.toml` scan ----------------------------

    private fun modelFromToml(base: File): Pair<JuxNode, List<DefaultMutableTreeNode>> {
        val rootManifest = File(base, "jux.toml")
        val rootLabel = JuxNode(base.name, AllIcons.Nodes.ModuleGroup, build = BuildContext(Ctx.WORKSPACE))
        val moduleDirs = discoverModules(base, rootManifest)
        val isWs = JuxToml.workspaceMembers(readFileOrEmpty(rootManifest)).isNotEmpty()
        val children = if (moduleDirs.isEmpty()) {
            listOf(DefaultMutableTreeNode(JuxNode("No jux.toml found", AllIcons.General.Information)))
        } else {
            moduleDirs.map { moduleNodeFromToml(it, isWs) }
        }
        return rootLabel to children
    }

    /**
     * Member module directories. A workspace root's `[workspace] members`
     * (a plain `dir`, or a trailing-glob form ending in slash-star) lists them;
     * otherwise the single root module (if it has a `jux.toml`).
     */
    private fun discoverModules(base: File, rootManifest: File): List<File> {
        val out = LinkedHashSet<File>()
        val text = readFileOrEmpty(rootManifest)
        val members = JuxToml.workspaceMembers(text)
        if (members.isNotEmpty()) {
            if (JuxToml.packageName(text) != null) out.add(base)
            for (m in members) {
                if (m.endsWith("/*")) {
                    val parent = File(base, m.removeSuffix("/*"))
                    parent.listFiles()?.filter { it.isDirectory && File(it, "jux.toml").isFile }
                        ?.sortedBy { it.name }?.forEach(out::add)
                } else {
                    val dir = File(base, m)
                    if (File(dir, "jux.toml").isFile) out.add(dir)
                }
            }
        } else if (rootManifest.isFile) {
            out.add(base)
        }
        return out.toList()
    }

    private fun moduleNodeFromToml(dir: File, isWorkspace: Boolean): DefaultMutableTreeNode {
        val manifest = File(dir, "jux.toml")
        val text = readFileOrEmpty(manifest)
        val name = JuxToml.packageName(text) ?: dir.name
        val isLib = JuxToml.hasLib(text)
        val bins = JuxToml.bins(text)
        val effectiveBins = when {
            bins.isNotEmpty() -> bins
            !isLib -> listOf(name)
            else -> emptyList()
        }
        val pkgSel = name.takeIf { isWorkspace }
        val node = DefaultMutableTreeNode(
            JuxNode(
                name,
                if (isLib) AllIcons.Nodes.PpLibFolder else AllIcons.Nodes.Module,
                tail = JuxToml.packageVersion(text),
                file = manifest.takeIf { it.isFile },
                build = BuildContext(Ctx.MODULE, pkg = pkgSel, runnable = effectiveBins.isNotEmpty()),
            ),
        )

        val targets = group("targets", AllIcons.Nodes.ModuleGroup)
        for (b in effectiveBins) {
            targets.add(
                DefaultMutableTreeNode(
                    JuxNode(b, AllIcons.Actions.Execute, tail = "bin",
                        build = BuildContext(Ctx.BIN, pkg = pkgSel, bin = b, runnable = true)),
                ),
            )
        }
        if (isLib) {
            val cts = JuxToml.libCrateTypes(text).joinToString(", ").ifEmpty { "lib" }
            targets.add(
                DefaultMutableTreeNode(
                    JuxNode(JuxToml.libName(text) ?: name, AllIcons.Nodes.PpLib, tail = cts,
                        build = BuildContext(Ctx.LIB, pkg = pkgSel, lib = true, runnable = false)),
                ),
            )
        }
        node.add(targets)

        JuxToml.edition(text)?.let { node.add(leaf("Edition", AllIcons.Nodes.Tag, tail = it)) }

        val deps = JuxToml.dependencyDetails(text)
        if (deps.isNotEmpty()) {
            val g = group("Dependencies", AllIcons.Nodes.PpLibFolder)
            for ((depName, detail) in deps) {
                g.add(leaf(depName, AllIcons.Nodes.PpLib, tail = detail.ifEmpty { null }))
            }
            node.add(g)
        }
        return node
    }

    // ---- Shared node helpers ----------------------------------------------

    private fun leaf(label: String, icon: Icon, tail: String? = null) =
        DefaultMutableTreeNode(JuxNode(label, icon, tail))

    private fun group(label: String, icon: Icon) =
        DefaultMutableTreeNode(JuxNode(label, icon))

    /** Expand the root, each module, and each module's `targets` group. */
    private fun expandModules() {
        tree.expandPath(TreePath(treeRoot.path))
        for (i in 0 until treeRoot.childCount) {
            val module = treeRoot.getChildAt(i) as DefaultMutableTreeNode
            tree.expandPath(TreePath(module.path))
            for (j in 0 until module.childCount) {
                val child = module.getChildAt(j) as DefaultMutableTreeNode
                if ((child.userObject as? JuxNode)?.label == "targets") {
                    tree.expandPath(TreePath(child.path))
                }
            }
        }
    }

    private fun readFileOrEmpty(f: File): String =
        try { if (f.isFile) f.readText() else "" } catch (_: Exception) { "" }
}

/** Read a string field, treating JSON null / blank as absent. */
private fun JsonObject.str(key: String): String? =
    get(key)?.takeIf { !it.isJsonNull }?.asString?.takeIf { it.isNotBlank() }

private fun JsonObject.bool(key: String): Boolean =
    get(key)?.takeIf { !it.isJsonNull }?.asBoolean ?: false

private class JuxModuleCellRenderer : com.intellij.ui.ColoredTreeCellRenderer() {
    override fun customizeCellRenderer(
        tree: javax.swing.JTree,
        value: Any?,
        selected: Boolean,
        expanded: Boolean,
        leaf: Boolean,
        row: Int,
        hasFocus: Boolean,
    ) {
        val payload = (value as? DefaultMutableTreeNode)?.userObject
        if (payload is JuxNode) {
            icon = payload.icon
            append(payload.label)
            payload.tail?.let { append("  $it", com.intellij.ui.SimpleTextAttributes.GRAYED_ATTRIBUTES) }
        } else {
            append(payload?.toString() ?: "")
        }
    }
}

/**
 * Minimal `jux.toml` reader — the **fallback** when `jux metadata` is
 * unavailable (older toolchain), plus the source for the default `[build]
 * target`. Section-aware line scan; tolerant of comments, quotes, and
 * whitespace; never throws.
 */
internal object JuxToml {
    fun packageName(text: String): String? = stringValueIn(text, "package", "name")
    fun packageVersion(text: String): String? = stringValueIn(text, "package", "version")
    fun edition(text: String): String? = stringValueIn(text, "package", "edition")

    /** Default cross-compile triple from `[build] target`, if declared. */
    fun buildTarget(text: String): String? = stringValueIn(text, "build", "target")

    /** True when the module declares a `[lib]` target (produces a library). */
    fun hasLib(text: String): Boolean = sectionBody(text, "lib") != null

    /** The `[lib] name`, if the manifest sets one explicitly. */
    fun libName(text: String): String? = stringValueIn(text, "lib", "name")

    /** `[lib] crate-type = [...]` (e.g. `lib`, `cdylib`); empty if unspecified. */
    fun libCrateTypes(text: String): List<String> {
        val body = sectionBody(text, "lib") ?: return emptyList()
        val m = Regex("""crate-type\s*=\s*\[(.*?)]""", RegexOption.DOT_MATCHES_ALL).find(body)
            ?: return emptyList()
        return Regex("\"([^\"]+)\"").findAll(m.groupValues[1]).map { it.groupValues[1] }.toList()
    }

    /** Names of every `[[bin]]` target the manifest declares. */
    fun bins(text: String): List<String> {
        val lines = text.lines()
        val header = Regex("""^\s*\[\[\s*bin\s*]]\s*$""")
        val out = ArrayList<String>()
        var i = 0
        while (i < lines.size) {
            if (!header.matches(lines[i].substringBefore('#'))) { i++; continue }
            var name: String? = null
            var j = i + 1
            while (j < lines.size && !lines[j].substringBefore('#').trim().startsWith("[")) {
                Regex("""^\s*name\s*=\s*"([^"]*)"""").find(lines[j])?.let { name = it.groupValues[1] }
                j++
            }
            name?.let(out::add)
            i = j
        }
        return out
    }

    /**
     * Dependency names: the inline keys under `[dependencies]` plus any
     * `[dependencies.NAME]` sub-tables (the dotted-table form for a dependency
     * with its own options).
     */
    fun dependencies(text: String): List<String> =
        (keysIn(text, "dependencies") + subTables(text, "dependencies").map { it.first }).distinct()

    /**
     * Each `[dependencies]` entry as `(name, sourceDetail)` — the detail is the
     * bare version (`"1.0"` → `1.0`), or `path: …` / `git: … (branch)` for
     * table specs, so the tree shows where each dependency comes from. Covers
     * both inline entries and `[dependencies.NAME]` sub-tables.
     */
    fun dependencyDetails(text: String): List<Pair<String, String>> {
        val out = ArrayList<Pair<String, String>>()
        sectionBody(text, "dependencies")?.let { body ->
            for (raw in body.lineSequence()) {
                val line = raw.substringBefore('#').trim()
                if (line.isEmpty() || line.startsWith("[") || '=' !in line) continue
                val key = line.substringBefore('=').trim().trim('"')
                if (key.isEmpty()) continue
                out.add(key to depDetail(line.substringAfter('=').trim()))
            }
        }
        for ((name, body) in subTables(text, "dependencies")) {
            val inline = "{" + body.replace(Regex("[\r\n]+"), " ").trim() + "}"
            out.add(name to depDetail(inline))
        }
        return out
    }

    private fun depDetail(value: String): String {
        if (value.startsWith("\"")) return value.trim().trim('"')
        fun field(name: String): String? =
            Regex("""\b$name\s*=\s*"([^"]*)"""").find(value)?.groupValues?.get(1)
        field("path")?.let { return "path: $it" }
        field("git")?.let { g ->
            val ref = field("branch") ?: field("tag") ?: field("rev")
            return "git: $g" + (if (ref != null) " ($ref)" else "")
        }
        field("version")?.let { return it }
        return value.trim('{', '}', ' ')
    }

    /**
     * Return [text] with [member] added to `[workspace] members`, or `null` when
     * it is already a member (no change needed). Creates the `[workspace]`
     * section, or the `members` array within it, when absent — so adding the
     * first module to a single-package project converts it into a workspace.
     */
    fun withWorkspaceMember(text: String, member: String): String? {
        if (member in workspaceMembers(text)) return null
        // No [workspace] at all → append the section.
        if (sectionBody(text, "workspace") == null) {
            val sep = if (text.isEmpty() || text.endsWith("\n")) "" else "\n"
            return text + sep + "\n[workspace]\nmembers = [\"$member\"]\n"
        }
        val array = Regex("""members\s*=\s*\[(.*?)]""", RegexOption.DOT_MATCHES_ALL).find(text)
        if (array == null) {
            // [workspace] present but no members array → add one after the header.
            val header = Regex("""(?m)^\s*\[\s*workspace\s*]\s*$""").find(text) ?: return null
            val nl = text.indexOf('\n', header.range.last)
            // When the header is the last line (no trailing newline), prepend one
            // so `members` doesn't get glued onto the `[workspace]` line.
            return if (nl < 0) {
                "$text\nmembers = [\"$member\"]\n"
            } else {
                text.substring(0, nl + 1) + "members = [\"$member\"]\n" + text.substring(nl + 1)
            }
        }
        val inner = array.groupValues[1].trimEnd().trimEnd(',')
        val updated = if (inner.isBlank()) "\"$member\"" else "$inner, \"$member\""
        return text.substring(0, array.range.first) + "members = [$updated]" + text.substring(array.range.last + 1)
    }

    /** `[workspace] members = [ ... ]` — the listed member paths (incl. globs). */
    fun workspaceMembers(text: String): List<String> {
        val section = sectionBody(text, "workspace") ?: return emptyList()
        val m = Regex("""members\s*=\s*\[(.*?)]""", RegexOption.DOT_MATCHES_ALL).find(section)
            ?: return emptyList()
        return Regex("\"([^\"]+)\"").findAll(m.groupValues[1]).map { it.groupValues[1] }.toList()
    }

    private fun stringValueIn(text: String, section: String, key: String): String? {
        val body = sectionBody(text, section) ?: return null
        val m = Regex("""(?m)^\s*${Regex.escape(key)}\s*=\s*"([^"]*)"""").find(body) ?: return null
        return m.groupValues[1].takeIf { it.isNotBlank() }
    }

    private fun keysIn(text: String, section: String): List<String> {
        val body = sectionBody(text, section) ?: return emptyList()
        val out = ArrayList<String>()
        for (raw in body.lineSequence()) {
            val line = raw.substringBefore('#').trim()
            if (line.isEmpty() || line.startsWith("[")) continue
            val key = line.substringBefore('=').trim().trim('"')
            if (key.isNotEmpty()) out.add(key)
        }
        return out
    }

    /**
     * Every `[section.NAME]` sub-table as `(NAME, body)`. TOML lets a dependency
     * carry its own options via a dotted header (`[dependencies.serde]`); the
     * plain [sectionBody] scan stops at the next `[`, so these would otherwise be
     * invisible to the tree. Each body runs to the next `[` header (or EOF).
     */
    private fun subTables(text: String, section: String): List<Pair<String, String>> {
        val lines = text.lines()
        val header = Regex("""^\s*\[\s*${Regex.escape(section)}\.([^\]]+?)\s*]\s*$""")
        val out = ArrayList<Pair<String, String>>()
        var i = 0
        while (i < lines.size) {
            val m = header.find(lines[i].substringBefore('#'))
            if (m == null) { i++; continue }
            val name = m.groupValues[1].trim().trim('"')
            val sb = StringBuilder()
            var j = i + 1
            while (j < lines.size && !lines[j].substringBefore('#').trim().startsWith("[")) {
                sb.appendLine(lines[j]); j++
            }
            if (name.isNotEmpty()) out.add(name to sb.toString())
            i = j
        }
        return out
    }

    /** The lines of `[section]` up to the next `[` header (or end of file). */
    private fun sectionBody(text: String, section: String): String? {
        val lines = text.lines()
        val header = Regex("""^\s*\[\s*${Regex.escape(section)}\s*]\s*$""")
        var start = -1
        for ((i, line) in lines.withIndex()) {
            if (header.matches(line.substringBefore('#'))) { start = i + 1; break }
        }
        if (start < 0) return null
        val sb = StringBuilder()
        for (i in start until lines.size) {
            if (lines[i].substringBefore('#').trim().startsWith("[")) break
            sb.appendLine(lines[i])
        }
        return sb.toString()
    }
}
