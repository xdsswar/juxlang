package dev.jux.intellij.resolve.hierarchy

import com.intellij.ide.hierarchy.HierarchyBrowser
import com.intellij.ide.hierarchy.HierarchyNodeDescriptor
import com.intellij.ide.hierarchy.HierarchyProvider
import com.intellij.ide.hierarchy.HierarchyTreeStructure
import com.intellij.ide.hierarchy.TypeHierarchyBrowserBase
import com.intellij.ide.util.treeView.NodeDescriptor
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxSubtypes
import dev.jux.intellij.resolve.JuxTypeIndex
import java.util.Comparator
import javax.swing.JPanel
import javax.swing.JTree

/**
 * Type Hierarchy (`Ctrl+H`) for Jux types — supertypes, subtypes, or both.
 *
 * All three views are composition over indexes the plugin already maintains:
 * [JuxHierarchy.superTypeNames] resolved through [JuxTypeIndex] walks up, and
 * [JuxSubtypes.subtypesOf] walks down. That is the same index behind the
 * override and subtype gutter icons, so the tree and the gutter can never
 * disagree about who implements what.
 */
class JuxTypeHierarchyProvider : HierarchyProvider {

    override fun getTarget(dataContext: DataContext): PsiElement? {
        val file = CommonDataKeys.PSI_FILE.getData(dataContext) as? JuxFile ?: return null
        val editor = CommonDataKeys.EDITOR.getData(dataContext)
        if (editor != null) {
            val at = file.findElementAt(editor.caretModel.offset)
            PsiTreeUtil.getParentOfType(at, JuxTypeDeclaration::class.java)?.let { return it }
        }
        // Invoked from the Project view or Structure view, where there is an
        // element but no editor caret.
        val element = CommonDataKeys.PSI_ELEMENT.getData(dataContext)
        return PsiTreeUtil.getParentOfType(element, JuxTypeDeclaration::class.java, false)
    }

    override fun createHierarchyBrowser(target: PsiElement): HierarchyBrowser =
        JuxTypeHierarchyBrowser(target.project, target)

    override fun browserActivated(hierarchyBrowser: HierarchyBrowser) {
        val browser = hierarchyBrowser as JuxTypeHierarchyBrowser
        // Open on the view that answers the question actually being asked: for
        // an interface, "who implements this?"; for a class, where it sits.
        browser.changeView(
            if (browser.isInterface) TypeHierarchyBrowserBase.getSubtypesHierarchyType()
            else TypeHierarchyBrowserBase.getTypeHierarchyType(),
        )
    }
}

/** The tool-window panel hosting the three Jux type-hierarchy trees. */
class JuxTypeHierarchyBrowser(project: Project, element: PsiElement) :
    TypeHierarchyBrowserBase(project, element) {

    override fun isInterface(psiElement: PsiElement): Boolean =
        psiElement is JuxTypeDeclaration && JuxHierarchy.isInterface(psiElement)

    /**
     * Delete is not offered from the hierarchy tree. Removing a type from a
     * window that exists to show what depends on it is a trap, and Safe Delete
     * from the editor is the supported route.
     */
    override fun canBeDeleted(psiElement: PsiElement?): Boolean = false

    override fun getQualifiedName(psiElement: PsiElement?): String =
        (psiElement as? JuxTypeDeclaration)?.name ?: ""

    override fun getElementFromDescriptor(descriptor: HierarchyNodeDescriptor): PsiElement? =
        descriptor.psiElement

    override fun isApplicableElement(element: PsiElement): Boolean = element is JuxTypeDeclaration

    override fun createHierarchyTreeStructure(type: String, psiElement: PsiElement): HierarchyTreeStructure? {
        val declaration = psiElement as? JuxTypeDeclaration ?: return null
        return when (type) {
            getSupertypesHierarchyType() -> JuxTypeHierarchyTreeStructure(declaration, Direction.SUPERTYPES)
            getSubtypesHierarchyType() -> JuxTypeHierarchyTreeStructure(declaration, Direction.SUBTYPES)
            getTypeHierarchyType() -> JuxTypeHierarchyTreeStructure(declaration, Direction.BOTH)
            else -> null
        }
    }

    override fun createTrees(trees: MutableMap<in String, in JTree>) {
        createTreeAndSetupCommonActions(trees, "JuxTypeHierarchyPopupMenu")
    }

    /**
     * No legend. The Java browser's legend explains its class/interface icon
     * pair; the Jux icons already carry the kind, so an extra strip of colour
     * keys would only take vertical space from the tree.
     */
    override fun createLegendPanel(): JPanel? = null

    override fun getComparator(): Comparator<NodeDescriptor<*>>? =
        JuxHierarchyNodeComparator

    override fun getPrevOccurenceActionNameImpl(): String = "Previous Type"

    override fun getNextOccurenceActionNameImpl(): String = "Next Type"
}

/** Which way a hierarchy tree walks from its root. */
internal enum class Direction { SUPERTYPES, SUBTYPES, BOTH }

/**
 * The tree behind one hierarchy view.
 *
 * `BOTH` is Java's "Type Hierarchy" view: the root is the topmost ancestor, so
 * the base type appears in context with its siblings, and children then walk
 * *down*. `SUPERTYPES` and `SUBTYPES` walk one way from the base type itself.
 */
internal class JuxTypeHierarchyTreeStructure(
    private val baseType: JuxTypeDeclaration,
    private val direction: Direction,
) : HierarchyTreeStructure(
    baseType.project,
    JuxHierarchyNodeDescriptor(baseType.project, null, rootFor(baseType, direction), isBase = true),
) {

    /**
     * supertype name → the types naming it, built once per tree.
     *
     * Built lazily and held for the life of the structure because the index is
     * a walk over every type in the project: rebuilding it on each node
     * expansion would make a deep tree quadratic in project size.
     */
    private val subtypeIndex by lazy { JuxSubtypes.buildIndex(baseType.project) }

    override fun buildChildren(descriptor: HierarchyNodeDescriptor): Array<Any> {
        val type = descriptor.psiElement as? JuxTypeDeclaration ?: return emptyArray()
        val children = when (direction) {
            Direction.SUPERTYPES -> supertypesOf(type)
            // Direct subtypes only: the tree expands a level at a time, so
            // handing it the transitive closure would list every descendant
            // beside its own parent as well as under it.
            Direction.SUBTYPES, Direction.BOTH -> JuxSubtypes.directSubtypes(type, subtypeIndex)
        }
        return children
            .map { JuxHierarchyNodeDescriptor(type.project, descriptor, it, isBase = false) }
            .toTypedArray()
    }

    private companion object {
        /**
         * The element the tree is rooted at: for the combined view, the topmost
         * ancestor, so the base type is shown where it actually sits rather
         * than as if it had no parents.
         */
        fun rootFor(base: JuxTypeDeclaration, direction: Direction): JuxTypeDeclaration {
            if (direction != Direction.BOTH) return base
            var root = base
            val seen = HashSet<String>()
            while (true) {
                // The cycle guard matters: a malformed `class A extends B` /
                // `class B extends A` pair is perfectly typable, and without
                // this the tree would spin instead of showing the mistake.
                val name = root.name ?: break
                if (!seen.add(name)) break
                root = supertypesOf(root).firstOrNull { !JuxHierarchy.isInterface(it) } ?: break
            }
            return root
        }

        /** The resolvable direct supertypes of [type], extends first. */
        fun supertypesOf(type: JuxTypeDeclaration): List<JuxTypeDeclaration> =
            JuxHierarchy.superTypeNames(type).mapNotNull { JuxTypeIndex.findType(type, it) }
    }
}

/** One node: a type declaration, rendered with its kind and name. */
internal class JuxHierarchyNodeDescriptor(
    project: Project,
    parent: NodeDescriptor<*>?,
    type: JuxTypeDeclaration,
    isBase: Boolean,
) : HierarchyNodeDescriptor(project, parent, type, isBase) {

    override fun update(): Boolean {
        val changed = super.update()
        val type = psiElement as? JuxTypeDeclaration ?: return changed
        val text = "${JuxHierarchy.kindNoun(type)} ${type.name}"
        if (myName != text) {
            myName = text
            return true
        }
        return changed
    }
}

/** Alphabetical ordering for the tree's "Sort Alphabetically" toggle. */
internal object JuxHierarchyNodeComparator : Comparator<NodeDescriptor<*>> {
    override fun compare(a: NodeDescriptor<*>, b: NodeDescriptor<*>): Int {
        val an = (a as? HierarchyNodeDescriptor)?.psiElement.let { (it as? JuxTypeDeclaration)?.name } ?: ""
        val bn = (b as? HierarchyNodeDescriptor)?.psiElement.let { (it as? JuxTypeDeclaration)?.name } ?: ""
        return an.compareTo(bn)
    }
}
