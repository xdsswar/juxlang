package dev.jux.intellij.project

import com.intellij.icons.AllIcons
import com.intellij.ide.projectView.PresentationData
import com.intellij.ide.projectView.TreeStructureProvider
import com.intellij.ide.projectView.ViewSettings
import com.intellij.ide.projectView.impl.nodes.PsiDirectoryNode
import com.intellij.ide.projectView.impl.nodes.PsiFileNode
import com.intellij.ide.util.treeView.AbstractTreeNode
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDirectory
import dev.jux.intellij.JuxPackageResolver

/**
 * The Project view's **Flatten Packages** for Jux (§I.4 "Package View").
 *
 * Java's flattening is built on its own package model, so it never touches a
 * Jux root: `src/com/example/app/` stays three nested folders. With the option
 * on, this provider shows a Jux package root the way Java shows one: one node
 * per package that holds files, labelled with the dotted name
 * (`com.example.app`), each listing only its own files. Packages with no
 * files are left out, as Java does.
 *
 * The other half of §I.4's Package View, collapsing a single-child chain of
 * directories, is the platform's **Compact Middle Packages** / **Compact
 * Directories** option, which works on any directory and so already applies to
 * Jux; nothing here is needed for it.
 *
 * Only roots the Jux layout vouches for are flattened (a marked root, or
 * `src/`/`test/` under a `jux.toml`), so a folder the plugin merely guessed at
 * is never relabelled.
 */
class JuxPackageTreeStructureProvider : TreeStructureProvider, DumbAware {

    override fun modify(
        parent: AbstractTreeNode<*>,
        children: Collection<AbstractTreeNode<*>>,
        settings: ViewSettings?,
    ): Collection<AbstractTreeNode<*>> {
        if (settings == null || !settings.isFlattenPackages) return children
        if (parent is JuxPackageNode) return children
        val directory = (parent as? PsiDirectoryNode)?.value ?: return children
        val project = parent.project ?: return children
        val root = JuxPackageResolver.rootFor(directory.virtualFile, project) ?: return children
        if (!root.authoritative || root.dir != directory.virtualFile) return children

        // The root's own files stay; every folder below becomes package nodes.
        val out = ArrayList<AbstractTreeNode<*>>()
        children.filterTo(out) { it !is PsiDirectoryNode }
        for (sub in directory.subdirectories.sortedBy { it.name }) collectPackages(project, root, sub, settings, out)
        return out
    }

    /** A node for [dir] when it holds files, then the same for each folder below it. */
    private fun collectPackages(
        project: Project,
        root: JuxPackageResolver.Root,
        dir: PsiDirectory,
        settings: ViewSettings,
        out: MutableList<AbstractTreeNode<*>>,
    ) {
        val pkg = JuxPackageResolver.packageUnder(root.dir, dir.virtualFile) ?: return
        val leaf = dir.subdirectories.isEmpty()
        if (dir.files.isNotEmpty() || (leaf && !settings.isHideEmptyMiddlePackages)) {
            out += JuxPackageNode(project, dir, settings, pkg)
        }
        for (sub in dir.subdirectories.sortedBy { it.name }) collectPackages(project, root, sub, settings, out)
    }
}

/**
 * One flattened package: the directory, shown by its dotted package name with
 * the package icon, and listing only its own files (the packages below it
 * are its siblings in a flattened tree, not its children).
 */
class JuxPackageNode(
    project: Project,
    directory: PsiDirectory,
    settings: ViewSettings,
    /** The dotted package name this node stands for. */
    val packageName: String,
) : PsiDirectoryNode(project, directory, settings) {

    override fun updateImpl(data: PresentationData) {
        super.updateImpl(data)
        data.presentableText = packageName
        data.locationString = null
        data.setIcon(AllIcons.Nodes.Package)
    }

    override fun getChildrenImpl(): Collection<AbstractTreeNode<*>> {
        val directory = value ?: return emptyList()
        val project = project ?: return emptyList()
        return directory.files.sortedBy { it.name }.map { PsiFileNode(project, it, settings) }
    }
}
