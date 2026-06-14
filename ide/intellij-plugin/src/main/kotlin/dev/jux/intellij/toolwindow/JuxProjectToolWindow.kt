package dev.jux.intellij.toolwindow

import com.intellij.icons.AllIcons
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.ActionToolbar
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.PopupHandler
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.content.ContentFactory
import com.intellij.ui.treeStructure.Tree
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
 * panel): a tree of the workspace's modules read from `jux.toml`, each expanding
 * to a **targets** group (its `[[bin]]` / `[lib]` artifacts), its edition and its
 * dependencies. Unlike a plain outline it is *action-driven* — the toolbar and a
 * right-click menu **build / run / test / check** whichever module or target is
 * selected, streaming into the shared [JuxConsoleService] console (the bottom
 * "Jux Build" window). A "Target" action picks the cross-compile triple.
 *
 * Because today's `jux` CLI resolves only the cwd's `jux.toml` (no `-p` / `--bin`
 * selector), per-module actions run with the working directory set to the
 * module's own directory, and per-binary Run notes when it can only launch the
 * default binary. The exact CLI gaps are catalogued in `cli-support-request.md`.
 */
class JuxProjectToolWindowFactory : ToolWindowFactory, DumbAware {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = JuxProjectPanel(project)
        val content = ContentFactory.getInstance().createContent(panel.root, "", false)
        toolWindow.contentManager.addContent(content)
    }
}

/** What a selected tree node lets you build/run, and where. */
private enum class Ctx { WORKSPACE, MODULE, BIN, LIB }

/**
 * The build context carried by an actionable node: the working directory the
 * `jux` command runs in, the kind of thing it is, whether it can be *run* (a
 * library cannot), and whether its module declares multiple binaries (so Run can
 * warn that only the default one launches).
 */
private data class BuildContext(
    val workDir: File,
    val kind: Ctx,
    val runnable: Boolean = true,
    val multiBin: Boolean = false,
)

/**
 * A tree node payload: primary [label], optional grayed [tail] (version / crate
 * type / source detail), [icon], an optional [file] to open on double-click, and
 * an optional [build] context that makes the node a build/run target.
 */
private data class JuxNode(
    val label: String,
    val icon: Icon,
    val tail: String? = null,
    val file: File? = null,
    val build: BuildContext? = null,
)

private class JuxProjectPanel(private val project: Project) {
    val root: JPanel = JPanel(BorderLayout())
    private val treeRoot = DefaultMutableTreeNode(JuxNode("Jux", AllIcons.Nodes.ModuleGroup))
    private val model = DefaultTreeModel(treeRoot)
    private val tree = Tree(model)

    /** Last computed workspace root, used as the fallback build context. */
    private var workspaceRoot: File? = null

    init {
        tree.isRootVisible = true
        tree.showsRootHandles = true
        tree.cellRenderer = JuxModuleCellRenderer()
        tree.addMouseListener(object : java.awt.event.MouseAdapter() {
            override fun mousePressed(e: java.awt.event.MouseEvent) {
                // Right-click selects the node under the cursor so the context
                // menu (installed below) acts on what was clicked.
                if (e.isPopupTrigger) selectRowAt(e)
            }

            override fun mouseReleased(e: java.awt.event.MouseEvent) {
                if (e.isPopupTrigger) selectRowAt(e)
            }

            override fun mouseClicked(e: java.awt.event.MouseEvent) {
                if (e.clickCount == 2) onDoubleClick()
            }
        })

        val actions = buildActionGroup()
        val toolbar: ActionToolbar =
            ActionManager.getInstance().createActionToolbar("JuxProject", actions, true)
        toolbar.targetComponent = root
        // Same actions as a right-click context menu, scoped to the clicked node.
        PopupHandler.installPopupMenu(tree, actions, "JuxProjectPopup")

        root.add(toolbar.component, BorderLayout.NORTH)
        root.add(JBScrollPane(tree), BorderLayout.CENTER)
        reload()
    }

    // ---- Actions -----------------------------------------------------------

    /** The shared toolbar / context-menu action set. */
    private fun buildActionGroup(): DefaultActionGroup = DefaultActionGroup().apply {
        add(buildAction("Build", "Compile the selected module / target", AllIcons.Actions.Compile, "build"))
        add(buildAction("Run", "Build and run the selection", AllIcons.Actions.Execute, "run", runOnly = true))
        add(buildAction("Build Release", "Optimized release build", AllIcons.Actions.RealIntentionBulb, "build", release = true))
        add(buildAction("Test", "Run the selection's tests", AllIcons.RunConfigurations.TestState.Run, "test"))
        add(buildAction("Check", "Type-check without building", AllIcons.Actions.CheckOut, "check"))
        addSeparator()
        add(CrossTargetAction())
        addSeparator()
        add(object : AnAction("Refresh", "Reload modules from jux.toml", AllIcons.Actions.Refresh) {
            override fun getActionUpdateThread() = ActionUpdateThread.EDT
            override fun actionPerformed(e: AnActionEvent) = reload()
        })
    }

    /**
     * One context-aware action. [verb] is the `jux` subcommand; [release] adds
     * `--release`; [runOnly] actions (Run) are disabled for non-runnable nodes
     * (libraries). Enablement and the command both follow the current selection.
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

    /** The "Build for <triple>" action — edits the persisted cross-compile target. */
    private inner class CrossTargetAction : AnAction() {
        override fun getActionUpdateThread() = ActionUpdateThread.EDT
        override fun update(e: AnActionEvent) {
            val triple = crossTarget()
            e.presentation.icon = AllIcons.General.Settings
            e.presentation.text = "Target: " + (triple ?: "host")
            e.presentation.description = "Set the cross-compile target triple for builds"
        }
        override fun actionPerformed(e: AnActionEvent) {
            val current = JuxSettings.getInstance().crossTarget
            val input = Messages.showInputDialog(
                project,
                "Rust target triple for `jux build --target` (blank = host):\n" +
                    "e.g. x86_64-pc-windows-msvc, x86_64-unknown-linux-gnu, aarch64-apple-darwin",
                "Jux Build Target",
                AllIcons.General.Settings,
                current,
                null,
            ) ?: return
            JuxSettings.getInstance().crossTarget = input.trim()
        }
    }

    /**
     * Build the command line and stream it through the shared console. `build`
     * carries the cross-compile triple (`jux build` is the only verb the CLI
     * accepts `--target` on); Run on a multi-binary module notes that only the
     * default binary launches (no `--bin` selector yet).
     */
    private fun execute(verb: String, release: Boolean, ctx: BuildContext) {
        val args = ArrayList<String>()
        args.add(verb)
        if (release) args.add("--release")
        if (verb == "build") crossTarget()?.let { args.add("--target"); args.add(it) }
        val notice = if (verb == "run" && ctx.multiBin) {
            "this module declares multiple [[bin]] targets; `jux run` builds all and launches the " +
                "default (first) one — selecting a specific binary needs CLI `--bin` support."
        } else {
            null
        }
        JuxConsoleService.getInstance(project).run("jux", args, ctx.workDir, notice)
    }

    /**
     * The effective build context: the selected node's own context, or — when a
     * non-actionable node (or nothing) is selected — the whole workspace, so the
     * toolbar can always build the project.
     */
    private fun currentContext(): BuildContext? {
        val node = (tree.lastSelectedPathComponent as? DefaultMutableTreeNode)?.userObject as? JuxNode
        node?.build?.let { return it }
        return workspaceRoot?.let { BuildContext(it, Ctx.WORKSPACE) }
    }

    /** The cross-compile triple: settings override, else the root `[build] target`. */
    private fun crossTarget(): String? {
        JuxSettings.getInstance().crossTarget.takeIf { it.isNotBlank() }?.let { return it }
        val base = workspaceRoot ?: return null
        return JuxToml.buildTarget(readFileOrEmpty(File(base, "jux.toml")))
    }

    private fun selectRowAt(e: java.awt.event.MouseEvent) {
        val row = tree.getClosestRowForLocation(e.x, e.y)
        if (row >= 0) tree.setSelectionRow(row)
    }

    /** Double-click: open a node's file, else run its default action. */
    private fun onDoubleClick() {
        val node = (tree.lastSelectedPathComponent as? DefaultMutableTreeNode)?.userObject as? JuxNode ?: return
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
     * Rebuild the module tree from the project's `jux.toml` files. The manifest
     * reads + directory walks run on a pooled thread (never the EDT — they'd
     * freeze the UI on a large or network-mounted project), then the tree model
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

    /** Pure (off-EDT) computation of the root label + module nodes from disk. */
    private fun computeModel(base: File?): Pair<JuxNode, List<DefaultMutableTreeNode>> {
        if (base == null || !base.isDirectory) {
            return JuxNode("No project", AllIcons.General.Information) to emptyList()
        }
        val rootManifest = File(base, "jux.toml")
        val rootLabel = JuxNode(
            base.name,
            AllIcons.Nodes.ModuleGroup,
            build = BuildContext(base, Ctx.WORKSPACE),
        )
        val moduleDirs = discoverModules(base, rootManifest)
        val children = if (moduleDirs.isEmpty()) {
            listOf(DefaultMutableTreeNode(JuxNode("No jux.toml found", AllIcons.General.Information)))
        } else {
            moduleDirs.map { buildModuleNode(it) }
        }
        return rootLabel to children
    }

    /**
     * Member module directories. A workspace root's `[workspace] members`
     * (a plain `dir`, or a trailing-glob form ending in slash-star) lists
     * them; otherwise the single root module (if it has a `jux.toml`).
     */
    private fun discoverModules(base: File, rootManifest: File): List<File> {
        val out = LinkedHashSet<File>()
        val text = readFileOrEmpty(rootManifest)
        val members = JuxToml.workspaceMembers(text)
        if (members.isNotEmpty()) {
            // Workspace root: a manifest with a [package] is itself a member too.
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

    /**
     * One module's subtree — its *structure* from `jux.toml`, not its files: the
     * module node (carrying its dir as the build cwd), a **targets** group of its
     * `[[bin]]` / `[lib]` artifacts, the edition, and its dependencies with each
     * one's source (version / path / git). A library module gets the lib icon.
     */
    private fun buildModuleNode(dir: File): DefaultMutableTreeNode {
        val manifest = File(dir, "jux.toml")
        val text = readFileOrEmpty(manifest)
        val name = JuxToml.packageName(text) ?: dir.name
        val isLib = JuxToml.hasLib(text)
        val bins = JuxToml.bins(text)
        // A plain executable with no explicit [[bin]] still has one synthesized
        // binary (named after the package) when a main entry file is present.
        val effectiveBins = when {
            bins.isNotEmpty() -> bins
            !isLib -> listOf(name)
            else -> emptyList()
        }
        val multiBin = effectiveBins.size > 1
        val runnable = effectiveBins.isNotEmpty()

        val node = DefaultMutableTreeNode(
            JuxNode(
                name,
                if (isLib) AllIcons.Nodes.PpLibFolder else AllIcons.Nodes.Module,
                tail = JuxToml.packageVersion(text),
                file = manifest.takeIf { it.isFile },
                build = BuildContext(dir, Ctx.MODULE, runnable = runnable, multiBin = multiBin),
            ),
        )

        // The "targets" group — bins first, then the library artifact.
        val targets = group("targets", AllIcons.Nodes.ModuleGroup)
        for (b in effectiveBins) {
            targets.add(
                DefaultMutableTreeNode(
                    JuxNode(
                        b,
                        AllIcons.Actions.Execute,
                        tail = "bin",
                        build = BuildContext(dir, Ctx.BIN, runnable = true, multiBin = multiBin),
                    ),
                ),
            )
        }
        if (isLib) {
            val cts = JuxToml.libCrateTypes(text).joinToString(", ").ifEmpty { "lib" }
            targets.add(
                DefaultMutableTreeNode(
                    JuxNode(
                        JuxToml.libName(text) ?: name,
                        AllIcons.Nodes.PpLib,
                        tail = cts,
                        build = BuildContext(dir, Ctx.LIB, runnable = false),
                    ),
                ),
            )
        }
        node.add(targets)

        // Edition.
        JuxToml.edition(text)?.let { node.add(leaf("Edition", AllIcons.Nodes.Tag, tail = it)) }

        // Dependencies with each one's source detail.
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
 * Minimal `jux.toml` reader — just enough for the module tree (no TOML
 * dependency). Section-aware line scan: `[package]` name/version/edition,
 * `[lib]`, `[[bin]]`, `[dependencies]`, `[workspace]` members, `[build]` target.
 * Tolerant of comments, quotes, and whitespace; never throws.
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
        // `[dependencies.NAME]` sub-tables: the detail comes from the version /
        // path / git keys inside the sub-table body, normalized to an inline
        // table so `depDetail` reads it the same way as an inline entry.
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
