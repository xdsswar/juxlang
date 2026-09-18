package dev.jux.intellij.project

import com.intellij.ide.projectView.ViewSettings
import com.intellij.ide.projectView.impl.nodes.PsiDirectoryNode
import com.intellij.ide.projectView.impl.nodes.PsiFileNode
import com.intellij.ide.util.treeView.AbstractTreeNode
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.psi.PsiManager
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The Project view's Flatten Packages for a Jux root (§I.4), and the Mark
 * Directory as actions.
 */
class JuxPackageTreeStructureProviderTest : BasePlatformTestCase() {

    private fun settings(flatten: Boolean, hideEmpty: Boolean = true) = object : ViewSettings {
        override fun isFlattenPackages(): Boolean = flatten
        override fun isHideEmptyMiddlePackages(): Boolean = hideEmpty
    }

    /** The flattened children of the directory at [path], labelled for comparison. */
    private fun flattened(path: String, settings: ViewSettings): List<String> {
        val dir = PsiManager.getInstance(project).findDirectory(myFixture.findFileInTempDir(path))!!
        val parent = PsiDirectoryNode(project, dir, settings)
        @Suppress("UNCHECKED_CAST")
        val children = parent.children as Collection<AbstractTreeNode<*>>
        return JuxPackageTreeStructureProvider().modify(parent, children, settings).map { node ->
            when (node) {
                is JuxPackageNode -> node.packageName + node.children.joinToString(",", " [", "]") {
                    (it as PsiFileNode).value!!.name
                }
                is PsiDirectoryNode -> "dir " + node.value!!.name
                is PsiFileNode -> node.value!!.name
                else -> node.toString()
            }
        }
    }

    private fun shop() {
        myFixture.addFileToProject("shop/jux.toml", "")
        myFixture.addFileToProject("shop/src/main.jux", "void main() {}")
        myFixture.addFileToProject("shop/src/com/acme/Cart.jux", "package com.acme;")
        myFixture.addFileToProject("shop/src/com/acme/Line.jux", "package com.acme;")
        myFixture.addFileToProject("shop/src/com/acme/model/Item.jux", "package com.acme.model;")
        myFixture.tempDirFixture.findOrCreateDir("shop/src/com/acme/empty")
    }

    fun testFlattenShowsOneNodePerPackageWithFiles() {
        shop()
        assertEquals(
            listOf("main.jux", "com.acme [Cart.jux,Line.jux]", "com.acme.model [Item.jux]"),
            flattened("shop/src", settings(flatten = true)),
        )
    }

    fun testAnEmptyLeafPackageShowsWhenNotHidden() {
        shop()
        assertTrue(flattened("shop/src", settings(flatten = true, hideEmpty = false)).any { it.startsWith("com.acme.empty") })
    }

    fun testTreeIsUntouchedWithoutFlatten() {
        shop()
        assertEquals(listOf("dir com", "main.jux"), flattened("shop/src", settings(flatten = false)).sorted())
    }

    fun testOnlyTheLayoutsOwnRootsAreFlattened() {
        shop()
        // Below the root: not a root, so nothing changes.
        assertEquals(listOf("dir acme"), flattened("shop/src/com", settings(flatten = true)))
        // A folder the plugin only guesses a root for.
        myFixture.addFileToProject("loose/pkg/A.jux", "")
        assertEquals(listOf("dir pkg"), flattened("loose", settings(flatten = true)))
    }

    fun testMarkDirectoryAsActionsAreRegistered() {
        val manager = ActionManager.getInstance()
        assertEquals("Jux Sources Root", manager.getAction("Jux.MarkSourcesRoot")?.templatePresentation?.text)
        assertEquals("Jux Test Sources Root", manager.getAction("Jux.MarkTestSourcesRoot")?.templatePresentation?.text)
    }
}
