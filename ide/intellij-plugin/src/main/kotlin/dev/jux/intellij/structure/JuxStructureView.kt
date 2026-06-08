package dev.jux.intellij.structure

import com.intellij.icons.AllIcons
import com.intellij.ide.projectView.PresentationData
import com.intellij.ide.structureView.StructureViewBuilder
import com.intellij.ide.structureView.StructureViewModel
import com.intellij.ide.structureView.StructureViewModelBase
import com.intellij.ide.structureView.StructureViewTreeElement
import com.intellij.ide.structureView.TreeBasedStructureViewBuilder
import com.intellij.ide.util.treeView.smartTree.SortableTreeElement
import com.intellij.ide.util.treeView.smartTree.TreeElement
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
                JuxStructureViewModel(psiFile)
        }
}

private class JuxStructureViewModel(file: PsiFile) :
    StructureViewModelBase(file, JuxStructureViewElement(file)),
    StructureViewModel.ElementInfoProvider {

    override fun isAlwaysShowsPlus(element: StructureViewTreeElement): Boolean =
        element.value is JuxFile

    override fun isAlwaysLeaf(element: StructureViewTreeElement): Boolean {
        val value = element.value
        // Fields and enum constants never have structural children.
        return value is PsiElement &&
            (value.elementType === E.FIELD_DECLARATION || value.elementType === E.ENUM_CONSTANT)
    }
}

private class JuxStructureViewElement(private val element: NavigatablePsiElement) :
    StructureViewTreeElement, SortableTreeElement {

    override fun getValue(): Any = element
    override fun navigate(requestFocus: Boolean) = element.navigate(requestFocus)
    override fun canNavigate(): Boolean = element.canNavigate()
    override fun canNavigateToSource(): Boolean = element.canNavigateToSource()

    override fun getAlphaSortKey(): String = (element as? PsiNamedElement)?.name.orEmpty()

    override fun getPresentation(): ItemPresentation {
        val text = when (element) {
            is JuxFile -> element.name
            is PsiNamedElement -> element.name ?: "<anonymous>"
            else -> element.text
        }
        return PresentationData(text, null, iconFor(element), null)
    }

    override fun getChildren(): Array<TreeElement> {
        val out = ArrayList<TreeElement>()
        collect(element, out)
        return out.toTypedArray()
    }

    /** Add each named declaration found beneath [parent], descending through
     *  non-declaration containers (the file root, class bodies) but not into the
     *  declarations themselves — they supply their own children. */
    private fun collect(parent: PsiElement, out: MutableList<TreeElement>) {
        for (child in parent.children) {
            if (child is JuxNamedElement && child is NavigatablePsiElement) {
                out.add(JuxStructureViewElement(child))
            } else {
                collect(child, out)
            }
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
