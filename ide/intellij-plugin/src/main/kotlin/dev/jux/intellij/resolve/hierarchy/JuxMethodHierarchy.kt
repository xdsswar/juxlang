package dev.jux.intellij.resolve.hierarchy

import com.intellij.icons.AllIcons
import com.intellij.ide.hierarchy.HierarchyBrowser
import com.intellij.ide.hierarchy.HierarchyBrowserManager
import com.intellij.ide.hierarchy.HierarchyNodeDescriptor
import com.intellij.ide.hierarchy.HierarchyProvider
import com.intellij.ide.hierarchy.HierarchyTreeStructure
import com.intellij.ide.hierarchy.MethodHierarchyBrowserBase
import com.intellij.ide.util.treeView.NodeDescriptor
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.ActionPlaces
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.actionSystem.IdeActions
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.ui.PopupHandler
import dev.jux.intellij.codeInsight.JuxGotoSuperHandler
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxSubtypes
import java.util.Comparator
import javax.swing.JPanel
import javax.swing.JTree

/**
 * Method Hierarchy (Ctrl+Shift+H): every type in the method's hierarchy, and
 * for each whether it defines the method, inherits it, or, being concrete
 * under an abstract declaration, must define it and does not. Java's view,
 * with Java's three icons and its "hide classes where the method is not
 * implemented" filter.
 *
 * The tree is rooted at the topmost type that declares the method, so the
 * selected declaration shows in context, and walks down through
 * [JuxSubtypes], the index behind the subtype gutter icons.
 */
class JuxMethodHierarchyProvider : HierarchyProvider {

    override fun getTarget(dataContext: DataContext): PsiElement? {
        val file = CommonDataKeys.PSI_FILE.getData(dataContext) as? JuxFile
        val editor = CommonDataKeys.EDITOR.getData(dataContext)
        val at = if (file != null && editor != null) file.findElementAt(editor.caretModel.offset)
        else CommonDataKeys.PSI_ELEMENT.getData(dataContext)
        val method = PsiTreeUtil.getParentOfType(at, JuxMethodDeclaration::class.java, false) ?: return null
        // Only a member method has a hierarchy: not a constructor, an operator
        // or a free function.
        if (method.elementType !== E.METHOD_DECLARATION || JuxHierarchy.enclosingType(method) == null) return null
        return method
    }

    override fun createHierarchyBrowser(target: PsiElement): HierarchyBrowser =
        JuxMethodHierarchyBrowser(target.project, target)

    override fun browserActivated(hierarchyBrowser: HierarchyBrowser) {
        (hierarchyBrowser as JuxMethodHierarchyBrowser).changeView(MethodHierarchyBrowserBase.getMethodType())
    }
}

/** The tool-window panel hosting the method-hierarchy tree. */
class JuxMethodHierarchyBrowser(project: Project, method: PsiElement) : MethodHierarchyBrowserBase(project, method) {

    override fun createTrees(trees: MutableMap<in String, in JTree>) {
        val tree = createTree(false)
        PopupHandler.installPopupMenu(tree, IdeActions.GROUP_METHOD_HIERARCHY_POPUP, ActionPlaces.METHOD_HIERARCHY_VIEW_POPUP)
        BaseOnThisMethodAction().registerCustomShortcutSet(
            ActionManager.getInstance().getAction(IdeActions.ACTION_METHOD_HIERARCHY).shortcutSet,
            tree,
        )
        trees[getMethodType()] = tree
    }

    override fun createLegendPanel(): JPanel = createStandardLegendPanel(
        "Method defined in the type",
        "Method not defined in the type",
        "Method should be defined because the type is not abstract",
    )

    /** The type a node stands for; the tree navigates to its declaration. */
    override fun getElementFromDescriptor(descriptor: HierarchyNodeDescriptor): PsiElement? =
        (descriptor as? JuxMethodNodeDescriptor)?.let { it.method ?: it.type } ?: descriptor.psiElement

    override fun isApplicableElement(element: PsiElement): Boolean =
        element is JuxMethodDeclaration && element.elementType === E.METHOD_DECLARATION

    override fun createHierarchyTreeStructure(type: String, psiElement: PsiElement): HierarchyTreeStructure? {
        if (type != getMethodType()) return null
        val method = psiElement as? JuxMethodDeclaration ?: return null
        return JuxMethodHierarchyTreeStructure(myProject, method)
    }

    override fun getComparator(): Comparator<NodeDescriptor<*>> =
        Comparator.comparing { d: NodeDescriptor<*> -> d.toString() }
}

/** How a type in the hierarchy relates to the method. */
enum class JuxMethodState {
    /** The type declares the method itself. */
    DEFINED,

    /** The type inherits a body for it. */
    INHERITED,

    /** A concrete type with only an abstract declaration above it. */
    SHOULD_DEFINE,
}

/** One node: a type, with the method it declares (if it does) and its state. */
class JuxMethodNodeDescriptor(
    project: Project,
    parent: NodeDescriptor<*>?,
    val type: JuxTypeDeclaration,
    val method: PsiElement?,
    val state: JuxMethodState,
    isBase: Boolean,
) : HierarchyNodeDescriptor(project, parent, type, isBase) {

    /**
     * Whether a type below this one inherits a BODY for the method: this
     * type's own declaration has one, or, declaring none, a type above did.
     */
    val bodyAvailable: Boolean =
        method?.let { JuxHierarchy.hasBody(it) } ?: ((parent as? JuxMethodNodeDescriptor)?.bodyAvailable == true)

    override fun update(): Boolean {
        val changed = super.update()
        icon = when (state) {
            JuxMethodState.DEFINED -> AllIcons.Hierarchy.MethodDefined
            JuxMethodState.INHERITED -> AllIcons.Hierarchy.MethodNotDefined
            JuxMethodState.SHOULD_DEFINE -> AllIcons.Hierarchy.ShouldDefineMethod
        }
        val text = "${JuxHierarchy.kindNoun(type)} ${type.name}"
        if (myName != text) {
            myName = text
            return true
        }
        return changed
    }
}

/** The method-hierarchy tree for one method. */
class JuxMethodHierarchyTreeStructure(project: Project, private val base: JuxMethodDeclaration) :
    HierarchyTreeStructure(project, rootDescriptor(project, base)) {

    private val name = base.name ?: ""
    private val arity = JuxHierarchy.arity(base)
    private val subtypeIndex by lazy { JuxSubtypes.buildIndex(project) }

    override fun buildChildren(descriptor: HierarchyNodeDescriptor): Array<Any> {
        val node = descriptor as? JuxMethodNodeDescriptor ?: return emptyArray()
        val hideInherited = HierarchyBrowserManager.getInstance(myProject).state?.HIDE_CLASSES_WHERE_METHOD_NOT_IMPLEMENTED == true
        return JuxSubtypes.directSubtypes(node.type, subtypeIndex)
            .filter { !hideInherited || definesBelow(it, HashSet()) }
            .map { sub ->
                val own = declared(sub, name, arity)
                val state = stateOf(sub, own, inheritsBody = node.bodyAvailable)
                JuxMethodNodeDescriptor(myProject, descriptor, sub, own, state, isBase = own == base)
            }.toTypedArray()
    }

    /** Whether [type] or a subtype declares the method (the filter's test). */
    private fun definesBelow(type: JuxTypeDeclaration, seen: MutableSet<String>): Boolean {
        if (declared(type, name, arity) != null) return true
        if (!seen.add(type.name ?: return false)) return false
        return JuxSubtypes.directSubtypes(type, subtypeIndex).any { definesBelow(it, seen) }
    }

    companion object {
        /** The method [type] itself declares with [name] and [arity], if any. */
        fun declared(type: JuxTypeDeclaration, name: String, arity: Int): PsiElement? =
            JuxHierarchy.directChildren(type, E.METHOD_DECLARATION).firstOrNull {
                (it as? JuxNamedElement)?.name == name && JuxHierarchy.arity(it) == arity
            }

        /**
         * A type's state: it defines the method, or it does not and either
         * inherits a body ([inheritsBody]) or, being concrete, should define
         * it. Abstract types and interfaces never "should".
         */
        fun stateOf(type: JuxTypeDeclaration, own: PsiElement?, inheritsBody: Boolean): JuxMethodState = when {
            own != null -> JuxMethodState.DEFINED
            inheritsBody || JuxHierarchy.isAbstractType(type) -> JuxMethodState.INHERITED
            else -> JuxMethodState.SHOULD_DEFINE
        }

        /** The topmost declaration of [base], and the root node for its type. */
        private fun rootDescriptor(project: Project, base: JuxMethodDeclaration): JuxMethodNodeDescriptor {
            var top: PsiElement = base
            val seen = HashSet<PsiElement>()
            while (top is JuxMethodDeclaration && seen.add(top)) {
                // Follow the class chain first, as Java's view does; an
                // interface declaration is the root when nothing is above it.
                val supers = JuxGotoSuperHandler.superMethods(top)
                top = supers.firstOrNull { s ->
                    JuxHierarchy.enclosingType(s)?.let { !JuxHierarchy.isInterface(it) } == true
                } ?: supers.firstOrNull() ?: break
            }
            val type = JuxHierarchy.enclosingType(top)!!
            return JuxMethodNodeDescriptor(project, null, type, top, JuxMethodState.DEFINED, isBase = top == base)
        }
    }
}
