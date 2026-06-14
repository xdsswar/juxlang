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
import com.intellij.openapi.vfs.LocalFileSystem
import com.intellij.openapi.wm.ToolWindow
import com.intellij.openapi.wm.ToolWindowFactory
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.content.ContentFactory
import com.intellij.ui.treeStructure.Tree
import java.awt.BorderLayout
import java.io.File
import javax.swing.JComponent
import javax.swing.JPanel
import javax.swing.event.TreeSelectionListener
import javax.swing.tree.DefaultMutableTreeNode
import javax.swing.tree.DefaultTreeModel
import javax.swing.tree.TreePath

/**
 * The **Jux** tool window (anchored right, like Cargo / Gradle): a tree of the
 * project's modules read from `jux.toml`. For a workspace it lists every
 * member package; for a single-module project it shows the one package. Each
 * module expands to its version, declared dependencies, and source roots.
 * Double-clicking a module opens its `jux.toml`.
 *
 * The build/run **console** lives in the separate bottom tool window
 * ([JuxToolWindowFactory]); this panel is purely the project structure.
 */
class JuxProjectToolWindowFactory : ToolWindowFactory, DumbAware {
    override fun createToolWindowContent(project: Project, toolWindow: ToolWindow) {
        val panel = JuxProjectPanel(project)
        val content = ContentFactory.getInstance().createContent(panel.root, "", false)
        toolWindow.contentManager.addContent(content)
    }
}

private class JuxProjectPanel(private val project: Project) {
    val root: JPanel = JPanel(BorderLayout())
    private val treeRoot = DefaultMutableTreeNode("Jux")
    private val model = DefaultTreeModel(treeRoot)
    private val tree = Tree(model)

    init {
        tree.isRootVisible = true
        tree.showsRootHandles = true
        tree.cellRenderer = JuxModuleCellRenderer()
        tree.addTreeSelectionListener(openManifestOnActivate())
        // Double-click a node tagged with a file → open it.
        tree.addMouseListener(object : java.awt.event.MouseAdapter() {
            override fun mouseClicked(e: java.awt.event.MouseEvent) {
                if (e.clickCount == 2) openSelectedFile()
            }
        })

        val group = DefaultActionGroup().apply {
            add(object : AnAction("Refresh", "Reload modules from jux.toml", AllIcons.Actions.Refresh) {
                override fun getActionUpdateThread() = ActionUpdateThread.EDT
                override fun actionPerformed(e: AnActionEvent) = reload()
            })
        }
        val toolbar: ActionToolbar =
            ActionManager.getInstance().createActionToolbar("JuxProject", group, false)
        toolbar.targetComponent = root
        root.add(toolbar.component, BorderLayout.WEST)
        root.add(JBScrollPane(tree), BorderLayout.CENTER)
        reload()
    }

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
                treeRoot.userObject = rootLabel
                treeRoot.removeAllChildren()
                for (c in children) treeRoot.add(c)
                model.reload()
                expandTopLevel()
            }
        }
    }

    /** Pure (off-EDT) computation of the root label + module nodes from disk. */
    private fun computeModel(base: File?): Pair<JuxNode, List<DefaultMutableTreeNode>> {
        if (base == null || !base.isDirectory) {
            return JuxNode("No project", AllIcons.General.Information, null) to emptyList()
        }
        val rootManifest = File(base, "jux.toml")
        val rootLabel = JuxNode(base.name, AllIcons.Nodes.ModuleGroup, null)
        val moduleDirs = discoverModules(base, rootManifest)
        val children = if (moduleDirs.isEmpty()) {
            listOf(DefaultMutableTreeNode(JuxNode("No jux.toml found", AllIcons.General.Information, null)))
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
     * One module's subtree — its *structure* from `jux.toml`, not its files:
     * the build kind (executable / library + crate-type), edition, any named
     * `[[bin]]` targets, and its dependencies with each one's source
     * (version / path / git). A library module gets the library icon.
     */
    private fun buildModuleNode(dir: File): DefaultMutableTreeNode {
        val manifest = File(dir, "jux.toml")
        val text = readFileOrEmpty(manifest)
        val name = JuxToml.packageName(text) ?: dir.name
        val isLib = JuxToml.hasLib(text)
        val node = DefaultMutableTreeNode(
            JuxNode(
                name,
                if (isLib) AllIcons.Nodes.PpLibFolder else AllIcons.Nodes.Module,
                manifest.takeIf { it.isFile },
                tail = JuxToml.packageVersion(text),
            ),
        )

        // Build kind.
        if (isLib) {
            val cts = JuxToml.libCrateTypes(text)
            node.add(leaf("Library", AllIcons.Nodes.PpLib, tail = cts.joinToString(", ").ifEmpty { "lib" }))
        } else {
            node.add(leaf("Executable", AllIcons.Actions.Execute))
        }

        // Edition.
        JuxToml.edition(text)?.let { node.add(leaf("Edition", AllIcons.Nodes.Tag, tail = it)) }

        // Named binary targets ([[bin]]).
        val bins = JuxToml.bins(text)
        if (bins.isNotEmpty()) {
            val g = group("Binaries", AllIcons.Nodes.ModuleGroup)
            for (b in bins) g.add(leaf(b, AllIcons.Actions.Execute))
            node.add(g)
        }

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

    private fun leaf(label: String, icon: javax.swing.Icon, tail: String? = null) =
        DefaultMutableTreeNode(JuxNode(label, icon, null, tail))

    private fun group(label: String, icon: javax.swing.Icon) =
        DefaultMutableTreeNode(JuxNode(label, icon, null))

    private fun expandTopLevel() {
        tree.expandPath(TreePath(treeRoot.path))
        for (i in 0 until treeRoot.childCount) {
            val child = treeRoot.getChildAt(i) as DefaultMutableTreeNode
            tree.expandPath(TreePath(child.path))
        }
    }

    private fun openManifestOnActivate(): TreeSelectionListener = TreeSelectionListener { /* open on double-click only */ }

    private fun openSelectedFile() {
        val node = tree.lastSelectedPathComponent as? DefaultMutableTreeNode ?: return
        val file = (node.userObject as? JuxNode)?.file ?: return
        val vf = LocalFileSystem.getInstance().findFileByIoFile(file) ?: return
        if (vf.isDirectory) return
        FileEditorManager.getInstance(project).openFile(vf, true)
    }

    private fun readFileOrEmpty(f: File): String =
        try { if (f.isFile) f.readText() else "" } catch (_: Exception) { "" }
}

/**
 * A tree node payload: primary label, optional grayed [tail] (version / source
 * detail), icon, and an optional file to open on double-click.
 */
private data class JuxNode(
    val label: String,
    val icon: javax.swing.Icon,
    val file: File?,
    val tail: String? = null,
)

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
 * dependency). Section-aware line scan: `[package]` name/version,
 * `[dependencies]` keys, `[workspace]` members. Tolerant of comments,
 * quotes, and whitespace; never throws.
 */
internal object JuxToml {
    fun packageName(text: String): String? = stringValueIn(text, "package", "name")
    fun packageVersion(text: String): String? = stringValueIn(text, "package", "version")
    fun edition(text: String): String? = stringValueIn(text, "package", "edition")

    /** True when the module declares a `[lib]` target (produces a library). */
    fun hasLib(text: String): Boolean = sectionBody(text, "lib") != null

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
