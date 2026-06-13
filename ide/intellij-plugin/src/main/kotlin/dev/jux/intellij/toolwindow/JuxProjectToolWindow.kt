package dev.jux.intellij.toolwindow

import com.intellij.icons.AllIcons
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.ActionToolbar
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.DefaultActionGroup
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

    /** Rebuild the module tree from the project's `jux.toml` files. */
    private fun reload() {
        treeRoot.removeAllChildren()
        val base = project.basePath?.let(::File)
        if (base == null || !base.isDirectory) {
            treeRoot.userObject = JuxNode("No project", AllIcons.General.Information, null)
            model.reload()
            return
        }
        val rootManifest = File(base, "jux.toml")
        treeRoot.userObject = JuxNode(base.name, AllIcons.Nodes.ModuleGroup, null)

        val moduleDirs = discoverModules(base, rootManifest)
        if (moduleDirs.isEmpty()) {
            treeRoot.add(DefaultMutableTreeNode(JuxNode("No jux.toml found", AllIcons.General.Information, null)))
        } else {
            for (dir in moduleDirs) treeRoot.add(buildModuleNode(dir))
        }
        model.reload()
        expandTopLevel()
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

    private fun buildModuleNode(dir: File): DefaultMutableTreeNode {
        val manifest = File(dir, "jux.toml")
        val text = readFileOrEmpty(manifest)
        val name = JuxToml.packageName(text) ?: dir.name
        val version = JuxToml.packageVersion(text)
        val label = if (version != null) "$name  $version" else name
        val node = DefaultMutableTreeNode(JuxNode(label, AllIcons.Nodes.Module, manifest.takeIf { it.isFile }))

        // Dependencies group.
        val deps = JuxToml.dependencies(text)
        if (deps.isNotEmpty()) {
            val depsNode = DefaultMutableTreeNode(JuxNode("Dependencies", AllIcons.Nodes.PpLibFolder, null))
            for (d in deps) {
                depsNode.add(DefaultMutableTreeNode(JuxNode(d, AllIcons.Nodes.PpLib, null)))
            }
            node.add(depsNode)
        }

        // Source roots that exist.
        for (srcName in listOf("src", "test")) {
            val srcDir = File(dir, srcName)
            if (srcDir.isDirectory) {
                node.add(DefaultMutableTreeNode(JuxNode(srcName, AllIcons.Nodes.Folder, srcDir)))
            }
        }
        return node
    }

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

/** A tree node payload: display label, icon, and an optional file to open. */
private data class JuxNode(val label: String, val icon: javax.swing.Icon, val file: File?)

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

    /** Dependency names = the keys under `[dependencies]`. */
    fun dependencies(text: String): List<String> = keysIn(text, "dependencies")

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
