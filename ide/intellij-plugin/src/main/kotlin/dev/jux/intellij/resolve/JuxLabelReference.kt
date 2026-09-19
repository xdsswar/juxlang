package dev.jux.intellij.resolve

import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiReferenceBase
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/**
 * The label of a `break outer;` / `continue outer;`, resolved to the nearest
 * enclosing labeled statement of that name (Grammar §A.2.8). A label is
 * visible only inside the statement it labels, so the walk goes outward from
 * the jump and stops at the enclosing function or lambda.
 */
class JuxLabelReference(element: PsiElement, range: TextRange) : PsiReferenceBase<PsiElement>(element, range) {

    override fun resolve(): PsiElement? {
        val label = value
        var p: PsiElement? = element.parent
        while (p != null) {
            when (p.elementType) {
                E.LABELED_STATEMENT -> if ((p as? JuxNamedElement)?.name == label) return p
                // A label does not reach into a nested function or lambda.
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION,
                E.LAMBDA_EXPRESSION -> return null
            }
            p = p.parent
        }
        return null
    }

    override fun handleElementRename(newElementName: String): PsiElement {
        val leaf = element.findElementAt(rangeInElement.startOffset) ?: return element
        leaf.replace(JuxElementFactory.createIdentifier(element.project, newElementName))
        return element
    }

    override fun getVariants(): Array<Any> = emptyArray()
}
