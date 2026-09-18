package dev.jux.intellij.resolve.hierarchy

import com.intellij.ide.hierarchy.CallHierarchyBrowserBase
import com.intellij.ide.hierarchy.HierarchyBrowser
import com.intellij.ide.hierarchy.HierarchyNodeDescriptor
import com.intellij.ide.hierarchy.HierarchyProvider
import com.intellij.ide.hierarchy.HierarchyTreeStructure
import com.intellij.ide.util.treeView.NodeDescriptor
import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.ActionPlaces
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.actionSystem.IdeActions
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.searches.ReferencesSearch
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.ui.PopupHandler
import dev.jux.intellij.codeInsight.JuxGotoSuperHandler
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import java.util.Comparator
import javax.swing.JTree

/**
 * Call Hierarchy (Ctrl+Alt+H) for Jux methods, constructors and functions:
 * who calls this (callers), and what this calls (callees).
 *
 * Callers come from [ReferencesSearch] over the plugin's own references, so
 * the tree finds exactly what Find Usages finds. Like Java's, a call made
 * through a super declaration counts: `shape.draw()` on a `Shape` reaches
 * `Circle.draw` at run time, so it is listed as one of its callers. Callees are
 * the calls and `new` expressions in the body, resolved the same way.
 */
class JuxCallHierarchyProvider : HierarchyProvider {

    override fun getTarget(dataContext: DataContext): PsiElement? {
        val file = CommonDataKeys.PSI_FILE.getData(dataContext) as? JuxFile
        val editor = CommonDataKeys.EDITOR.getData(dataContext)
        if (file != null && editor != null) {
            return JuxCallHierarchy.callableAt(file.findElementAt(editor.caretModel.offset))
        }
        return JuxCallHierarchy.callableAt(CommonDataKeys.PSI_ELEMENT.getData(dataContext))
    }

    override fun createHierarchyBrowser(target: PsiElement): HierarchyBrowser =
        JuxCallHierarchyBrowser(target.project, target)

    override fun browserActivated(hierarchyBrowser: HierarchyBrowser) {
        (hierarchyBrowser as JuxCallHierarchyBrowser).changeView(CallHierarchyBrowserBase.getCallerType())
    }
}

/** The tool-window panel hosting the caller and callee trees. */
class JuxCallHierarchyBrowser(project: Project, method: PsiElement) : CallHierarchyBrowserBase(project, method) {

    override fun createTrees(trees: MutableMap<in String, in JTree>) {
        for (type in listOf(getCalleeType(), getCallerType())) {
            val tree = createTree(false)
            PopupHandler.installPopupMenu(tree, IdeActions.GROUP_CALL_HIERARCHY_POPUP, ActionPlaces.CALL_HIERARCHY_VIEW_POPUP)
            BaseOnThisMethodAction().registerCustomShortcutSet(
                ActionManager.getInstance().getAction(IdeActions.ACTION_CALL_HIERARCHY).shortcutSet,
                tree,
            )
            trees[type] = tree
        }
    }

    override fun getElementFromDescriptor(descriptor: HierarchyNodeDescriptor): PsiElement? =
        descriptor.psiElement

    /** Double-click opens the call site, not the called declaration. */
    override fun getOpenFileElementFromDescriptor(descriptor: HierarchyNodeDescriptor): PsiElement? =
        (descriptor as? JuxCallNodeDescriptor)?.callSite ?: descriptor.psiElement

    override fun isApplicableElement(element: PsiElement): Boolean = JuxCallHierarchy.isCallable(element)

    override fun createHierarchyTreeStructure(type: String, psiElement: PsiElement): HierarchyTreeStructure? =
        when (type) {
            getCallerType() -> JuxCallerTreeStructure(myProject, psiElement)
            getCalleeType() -> JuxCalleeTreeStructure(myProject, psiElement)
            else -> null
        }

    override fun getComparator(): Comparator<NodeDescriptor<*>> =
        Comparator.comparing { d: NodeDescriptor<*> -> d.toString() }
}

/** Shared questions the two trees ask about callables. */
object JuxCallHierarchy {
    /** A method, constructor, operator or free function. */
    fun isCallable(element: PsiElement?): Boolean = element is JuxMethodDeclaration

    /** The callable enclosing [element] (or [element] itself). */
    fun callableAt(element: PsiElement?): PsiElement? =
        PsiTreeUtil.getParentOfType(element, JuxMethodDeclaration::class.java, false)

    /**
     * Readable name of a callable: `Type.name(int, String)`, the type's own
     * name for a constructor, and the bare name for a free function.
     */
    fun label(callable: PsiElement): String {
        val method = callable as? JuxMethodDeclaration ?: return callable.text.take(40)
        val owner = JuxHierarchy.enclosingType(method)?.name
        val name = when (method.elementType) {
            E.CONSTRUCTOR_DECLARATION -> owner ?: "constructor"
            E.OPERATOR_DECLARATION -> "operator" + (operatorSymbol(method) ?: "")
            else -> method.name ?: "?"
        }
        val params = JuxHierarchy.parameters(method).joinToString(", ") {
            it.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim() ?: "?"
        }
        val qualified = if (owner != null && method.elementType !== E.CONSTRUCTOR_DECLARATION) "$owner.$name" else name
        return "$qualified($params)"
    }

    private fun operatorSymbol(method: PsiElement): String? {
        var leaf = method.node.findChildByType(dev.jux.intellij.highlight.JuxTokenTypes.OPERATOR_KW)?.treeNext
        while (leaf != null && leaf.psi is com.intellij.psi.PsiWhiteSpace) leaf = leaf.treeNext
        return leaf?.text
    }

    /**
     * The places that call [callable], each with the callable it sits in: the
     * references to it, and to the methods it overrides, since a call through
     * the base type dispatches here too. Calls outside any callable (a field
     * initializer) are grouped under their type.
     */
    fun callers(callable: PsiElement): Map<PsiElement, List<PsiElement>> {
        val targets = ArrayList<PsiElement>()
        targets.add(callable)
        if (callable is JuxMethodDeclaration && callable.elementType === E.METHOD_DECLARATION) {
            targets.addAll(allSuperMethods(callable))
        }
        val scope = GlobalSearchScope.projectScope(callable.project)
        val out = LinkedHashMap<PsiElement, MutableList<PsiElement>>()
        for (target in targets) {
            for (ref in ReferencesSearch.search(target, scope).findAll()) {
                val site = ref.element
                val container = callableAt(site)
                    ?: PsiTreeUtil.getParentOfType(site, JuxTypeDeclaration::class.java)
                    ?: continue
                out.getOrPut(container) { ArrayList() }.add(site)
            }
        }
        return out
    }

    /** Every method [method] overrides, all the way up. */
    private fun allSuperMethods(method: JuxMethodDeclaration): List<PsiElement> {
        val out = LinkedHashSet<PsiElement>()
        val queue = ArrayDeque<JuxMethodDeclaration>()
        queue.add(method)
        while (queue.isNotEmpty()) {
            for (sup in JuxGotoSuperHandler.superMethods(queue.removeFirst())) {
                if (out.add(sup) && sup is JuxMethodDeclaration) queue.add(sup)
            }
        }
        return out.toList()
    }

    /**
     * The callables [callable] calls, each with its call sites, in the order
     * of their first call: every resolvable call and `new` in its body.
     */
    fun callees(callable: PsiElement): Map<PsiElement, List<PsiElement>> {
        val body = callable.node.findChildByType(E.CODE_BLOCK)?.psi ?: return emptyMap()
        val out = LinkedHashMap<PsiElement, MutableList<PsiElement>>()
        PsiTreeUtil.processElements(body) { e ->
            when (e.elementType) {
                E.CALL_EXPRESSION -> {
                    val callee = e.firstChild
                    val target = callee?.references?.firstNotNullOfOrNull { it.resolve() }
                    if (isCallable(target)) out.getOrPut(target!!) { ArrayList() }.add(e)
                }
                E.NEW_EXPRESSION -> constructorFor(e)?.let { out.getOrPut(it) { ArrayList() }.add(e) }
            }
            true
        }
        return out
    }

    /** The constructor a `new T(args)` runs: the one with that many parameters. */
    private fun constructorFor(newExpression: PsiElement): PsiElement? {
        val typeRef = newExpression.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return null
        val type = typeRef.references.firstNotNullOfOrNull { it.resolve() } as? JuxTypeDeclaration ?: return null
        val arity = dev.jux.intellij.resolve.JuxTypeEngine.argumentCount(newExpression)
        return JuxHierarchy.directChildren(type, E.CONSTRUCTOR_DECLARATION)
            .firstOrNull { JuxHierarchy.arity(it) == arity }
    }
}

/**
 * One node: a callable, with the call sites that connect it to its parent.
 * [callSite] is the first of them, where a double-click lands.
 */
class JuxCallNodeDescriptor(
    project: Project,
    parent: NodeDescriptor<*>?,
    element: PsiElement,
    isBase: Boolean,
    val sites: List<PsiElement>,
) : HierarchyNodeDescriptor(project, parent, element, isBase) {

    val callSite: PsiElement? get() = sites.firstOrNull()

    override fun update(): Boolean {
        val changed = super.update()
        val element = psiElement ?: return changed
        val count = sites.size
        val text = JuxCallHierarchy.label(element) + if (count > 1) "  ($count usages)" else ""
        if (myName != text) {
            myName = text
            return true
        }
        return changed
    }
}

/** Base for the two trees: children of a node, never repeating an ancestor. */
abstract class JuxCallTreeStructure(project: Project, root: PsiElement) :
    HierarchyTreeStructure(project, JuxCallNodeDescriptor(project, null, root, true, emptyList())) {

    /** The linked callables of [element], each with its call sites. */
    protected abstract fun linked(element: PsiElement): Map<PsiElement, List<PsiElement>>

    override fun buildChildren(descriptor: HierarchyNodeDescriptor): Array<Any> {
        val element = descriptor.psiElement ?: return emptyArray()
        // Recursion stops at the node that repeats an ancestor, as Java's
        // does: `a` calling `b` calling `a` shows `a` once more, unexpanded.
        var ancestor = descriptor.parentDescriptor
        while (ancestor != null) {
            if ((ancestor as? HierarchyNodeDescriptor)?.psiElement == element) return emptyArray()
            ancestor = ancestor.parentDescriptor
        }
        return linked(element).map { (target, sites) ->
            JuxCallNodeDescriptor(myProject, descriptor, target, false, sites)
        }.toTypedArray()
    }
}

/** Who calls the root. */
class JuxCallerTreeStructure(project: Project, root: PsiElement) : JuxCallTreeStructure(project, root) {
    override fun linked(element: PsiElement) = JuxCallHierarchy.callers(element)
}

/** What the root calls. */
class JuxCalleeTreeStructure(project: Project, root: PsiElement) : JuxCallTreeStructure(project, root) {
    override fun linked(element: PsiElement) = JuxCallHierarchy.callees(element)
}
