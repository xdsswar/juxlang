package dev.jux.intellij.structure

import com.intellij.icons.AllIcons
import com.intellij.ide.projectView.PresentationData
import com.intellij.ide.structureView.StructureViewBuilder
import com.intellij.ide.structureView.StructureViewModel
import com.intellij.ide.structureView.StructureViewModelBase
import com.intellij.ide.structureView.StructureViewTreeElement
import com.intellij.ide.structureView.TreeBasedStructureViewBuilder
import com.intellij.ide.util.InheritedMembersNodeProvider
import com.intellij.ide.util.treeView.smartTree.ActionPresentation
import com.intellij.ide.util.treeView.smartTree.ActionPresentationData
import com.intellij.ide.util.treeView.smartTree.Filter
import com.intellij.ide.util.treeView.smartTree.NodeProvider
import com.intellij.ide.util.treeView.smartTree.SortableTreeElement
import com.intellij.ide.util.treeView.smartTree.Sorter
import com.intellij.ide.util.treeView.smartTree.TreeElement
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeEngine
import com.intellij.lang.PsiStructureViewFactory
import com.intellij.navigation.ItemPresentation
import com.intellij.openapi.editor.Editor
import com.intellij.psi.NavigatablePsiElement
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiNamedElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.JuxIcons
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import javax.swing.Icon

/**
 * Drives the **Structure View** / File-Structure popup (`Ctrl+F12`) and the
 * breadcrumb outline from the PSI tree: top-level types and free functions,
 * nested members (fields, methods, constructors, enum constants), recursively.
 */
class JuxStructureViewFactory : PsiStructureViewFactory {
    override fun getStructureViewBuilder(psiFile: PsiFile): StructureViewBuilder =
        object : TreeBasedStructureViewBuilder() {
            override fun createStructureViewModel(editor: Editor?): StructureViewModel =
                JuxStructureViewModel(psiFile, editor)
        }
}

internal class JuxStructureViewModel(file: PsiFile, editor: Editor? = null) :
    StructureViewModelBase(file, editor, JuxStructureViewElement(file)),
    StructureViewModel.ElementInfoProvider {

    init {
        // Keep the tree's selection on the declaration the caret is in, as
        // Java's does ("Autoscroll from Source").
        withSuitableClasses(JuxNamedElement::class.java)
    }

    /** Only what the outline lists: a local or a parameter is no place to select. */
    override fun isSuitable(element: PsiElement?): Boolean =
        element != null && element.elementType in JuxStructureViewElement.MEMBER_TYPES && super.isSuitable(element)

    override fun isAlwaysShowsPlus(element: StructureViewTreeElement): Boolean =
        element.value is JuxFile

    override fun isAlwaysLeaf(element: StructureViewTreeElement): Boolean {
        val value = element.value
        // Fields and enum constants never have structural children.
        return value is PsiElement &&
            (value.elementType === E.FIELD_DECLARATION || value.elementType === E.ENUM_CONSTANT)
    }

    /** Java's toggles, plus one for properties: Ctrl+F12's "Show Fields", "Show Properties", "Show Non-Public". */
    override fun getFilters(): Array<Filter> = arrayOf(JuxFieldsFilter, JuxPropertiesFilter, JuxPublicElementsFilter)

    override fun getSorters(): Array<Sorter> = arrayOf(Sorter.ALPHA_SORTER)

    /** "Show Inherited" (Ctrl+F12 twice): the members a type gets from its supertypes. */
    override fun getNodeProviders(): Collection<NodeProvider<*>> = NODE_PROVIDERS

    private companion object {
        val NODE_PROVIDERS: Collection<NodeProvider<*>> = listOf(JuxInheritedMembersNodeProvider())
    }
}

/**
 * The members a type inherits and does not override, for "Show Inherited".
 * Each shows where it comes from (`toString ↑Base`), as Java's do.
 */
internal class JuxInheritedMembersNodeProvider : InheritedMembersNodeProvider<TreeElement>() {
    override fun provideNodes(node: TreeElement): Collection<TreeElement> {
        val decl = (node as? JuxStructureViewElement)?.value as? JuxTypeDeclaration ?: return emptyList()
        if (node.inheritedFrom != null) return emptyList()
        val out = ArrayList<TreeElement>()
        for (member in JuxTypeEngine.membersOf(JuxTypeEngine.selfType(decl))) {
            if (member.owner.decl == decl) continue
            val m = member.element
            if (m.elementType !in INHERITABLE || m !is NavigatablePsiElement) continue
            if (!JuxHierarchy.memberVisibleFrom(m, decl)) continue
            out.add(JuxStructureViewElement(m, inheritedFrom = member.owner.decl.name ?: "?"))
        }
        return out
    }

    private companion object {
        /** What a type passes on: not its constructors, and not its nested types. */
        val INHERITABLE = setOf(
            E.METHOD_DECLARATION, E.OPERATOR_DECLARATION, E.FIELD_DECLARATION,
            E.CONST_DECLARATION, E.PROPERTY_DECLARATION,
        )
    }
}

/** A structure filter over the element type of each node. */
private abstract class JuxKindFilter(
    private val id: String,
    private val text: String,
    private val icon: Icon,
) : Filter {
    abstract fun hides(element: PsiElement): Boolean

    override fun isVisible(treeNode: TreeElement): Boolean {
        val element = (treeNode as? JuxStructureViewElement)?.value as? PsiElement ?: return true
        return !hides(element)
    }

    override fun getPresentation(): ActionPresentation = ActionPresentationData(text, null, icon)
    override fun getName(): String = id

    /** Checked means shown, as Java's "Show Fields" reads. */
    override fun isReverted(): Boolean = true
}

/** "Show Fields": fields and constants. */
private object JuxFieldsFilter : JuxKindFilter("SHOW_FIELDS", "Show Fields", AllIcons.Nodes.Field) {
    override fun hides(element: PsiElement): Boolean =
        element.elementType === E.FIELD_DECLARATION || element.elementType === E.CONST_DECLARATION
}

/** "Show Properties": `public int Size { get; set; }` and its kin (§P). */
private object JuxPropertiesFilter : JuxKindFilter("SHOW_PROPERTIES", "Show Properties", AllIcons.Nodes.Property) {
    override fun hides(element: PsiElement): Boolean = element.elementType === E.PROPERTY_DECLARATION
}

/**
 * "Show Non-Public": members not declared `public`. An interface's members
 * are public without the keyword, and a top-level declaration is never
 * hidden, so the outline keeps its shape.
 */
private object JuxPublicElementsFilter : JuxKindFilter("SHOW_NON_PUBLIC", "Show Non-Public", AllIcons.Nodes.C_private) {
    override fun hides(element: PsiElement): Boolean {
        if (element.parent is JuxFile || element.elementType === E.ENUM_CONSTANT) return false
        val owner = PsiTreeUtil.getParentOfType(element, JuxTypeDeclaration::class.java)
        if (owner?.elementType === E.INTERFACE_DECLARATION) return false
        return !JuxHierarchy.hasModifier(element, "public")
    }
}

internal class JuxStructureViewElement(
    private val element: NavigatablePsiElement,
    /** The supertype a "Show Inherited" member comes from; null for the type's own. */
    val inheritedFrom: String? = null,
) : StructureViewTreeElement, SortableTreeElement {

    override fun getValue(): Any = element
    override fun navigate(requestFocus: Boolean) = element.navigate(requestFocus)
    override fun canNavigate(): Boolean = element.canNavigate()
    override fun canNavigateToSource(): Boolean = element.canNavigateToSource()

    override fun getAlphaSortKey(): String = (element as? PsiNamedElement)?.name.orEmpty()

    override fun getPresentation(): ItemPresentation {
        val text = when {
            element is JuxFile -> element.name
            // A constructor reads as its type's name, as Java's does; a
            // `new(...)` constructor has no name of its own to show.
            element.elementType === E.CONSTRUCTOR_DECLARATION ->
                (element as? PsiNamedElement)?.name ?: enclosingTypeName(element) ?: "new(...)"
            // An operator reads as itself: `operator==`, `operator hash`.
            element.elementType === E.OPERATOR_DECLARATION -> operatorLabel(element)
            element is PsiNamedElement -> element.name ?: "<anonymous>"
            else -> element.text
        }
        return PresentationData(text, inheritedFrom?.let { "↑$it" }, iconFor(element), null)
    }

    override fun getChildren(): Array<TreeElement> {
        // An inherited member is listed, not expanded: its own members are
        // shown where it is declared.
        if (inheritedFrom != null) return emptyArray()
        val out = ArrayList<TreeElement>()
        collect(element, out)
        return out.toTypedArray()
    }

    /** Add each MEMBER-level named declaration found beneath [parent],
     *  descending through non-declaration containers (the file root, class
     *  bodies) but not into the declarations themselves — they supply their own
     *  children. Parameters and local variables are named elements too, but
     *  they're implementation detail, not structure: the outline shows types,
     *  fields, methods, constructors, and enum constants only. */
    private fun collect(parent: PsiElement, out: MutableList<TreeElement>) {
        for (child in parent.children) {
            val type = child.elementType
            when {
                type in MEMBER_TYPES && child is NavigatablePsiElement ->
                    out.add(JuxStructureViewElement(child))
                // Don't descend into method/constructor bodies — locals and
                // parameters stay out of the outline.
                type in NO_DESCEND -> {}
                child is JuxNamedElement -> {} // param/local at this level: skip
                else -> collect(child, out)
            }
        }
    }

    companion object {
        /** Declarations the outline lists. */
        val MEMBER_TYPES = setOf(
            E.CLASS_DECLARATION, E.STRUCT_DECLARATION, E.INTERFACE_DECLARATION,
            E.ENUM_DECLARATION, E.RECORD_DECLARATION, E.ANNOTATION_DECLARATION,
            E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION,
            E.FIELD_DECLARATION, E.CONST_DECLARATION, E.PROPERTY_DECLARATION,
            E.ENUM_CONSTANT, E.TYPE_ALIAS_DECLARATION,
        )

        /** Declarations a constructor can belong to. */
        val TYPE_DECLARATIONS = setOf(
            E.CLASS_DECLARATION, E.STRUCT_DECLARATION, E.ENUM_DECLARATION, E.RECORD_DECLARATION,
        )

        /** Containers whose interiors are bodies, not structure. */
        val NO_DESCEND = setOf(
            E.CODE_BLOCK, E.INIT_BLOCK, E.STATIC_BLOCK, E.DROP_BLOCK,
            E.PARAMETER_LIST,
        )
    }

    /** The name of the type [e] is declared in, if any. */
    private fun enclosingTypeName(e: PsiElement): String? {
        var p = e.parent
        while (p != null && p !is JuxFile) {
            if (p.elementType in TYPE_DECLARATIONS) return (p as? PsiNamedElement)?.name
            p = p.parent
        }
        return null
    }

    /**
     * `operator` followed by what it overloads: a symbol hugs the keyword
     * (`operator==`, `operator[]`), a named operator takes a space
     * (`operator hash`, `operator string`).
     */
    private fun operatorLabel(e: PsiElement): String {
        var leaf = e.node.findChildByType(dev.jux.intellij.highlight.JuxTokenTypes.OPERATOR_KW)?.treeNext
        while (leaf != null && leaf.psi is com.intellij.psi.PsiWhiteSpace) leaf = leaf.treeNext
        val symbol = StringBuilder()
        // `[]`, `[]=` and `()` are several tokens; stop at the parameter list.
        while (leaf != null && leaf.elementType !== E.PARAMETER_LIST && leaf.psi !is com.intellij.psi.PsiWhiteSpace) {
            symbol.append(leaf.text)
            leaf = leaf.treeNext
            if (symbol.firstOrNull()?.isLetter() == true) break
        }
        val text = symbol.toString()
        return when {
            text.isEmpty() -> "operator"
            text.first().isLetter() -> "operator $text"
            else -> "operator$text"
        }
    }

    private fun iconFor(e: PsiElement): Icon? = when (e.elementType) {
        E.CLASS_DECLARATION, E.STRUCT_DECLARATION -> AllIcons.Nodes.Class
        E.INTERFACE_DECLARATION -> AllIcons.Nodes.Interface
        E.ENUM_DECLARATION -> AllIcons.Nodes.Enum
        E.RECORD_DECLARATION -> AllIcons.Nodes.Record
        E.ANNOTATION_DECLARATION -> AllIcons.Nodes.Annotationtype
        E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION -> AllIcons.Nodes.Method
        E.FIELD_DECLARATION, E.CONST_DECLARATION, E.PROPERTY_DECLARATION -> AllIcons.Nodes.Field
        E.ENUM_CONSTANT -> AllIcons.Nodes.Enum
        else -> if (e is JuxFile) JuxIcons.FILE else null
    }
}
