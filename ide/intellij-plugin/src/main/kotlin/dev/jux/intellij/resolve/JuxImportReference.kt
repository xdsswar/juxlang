package dev.jux.intellij.resolve

import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiReferenceBase
import dev.jux.intellij.psi.JuxElementFactory

/**
 * A type named by an `import`: `import some.Truck;`, or one item of
 * `import some.{Auto, Bus as B};`.
 *
 * An import is a use of the type like any other: go-to lands on the class,
 * find-usages counts it, and renaming the class rewrites it. Without a
 * reference here a rename left every importing file pointing at a name that no
 * longer existed.
 */
class JuxImportReference(
    element: PsiElement,
    range: TextRange,
    private val packagePath: String,
    private val typeName: String,
) : PsiReferenceBase<PsiElement>(element, range) {

    /** A package segment, a wildcard or a crate path may name nothing the index knows. */
    override fun isSoft(): Boolean = true

    override fun resolve(): PsiElement? = JuxTypeEngine.findTypeByFqn(element, packagePath, typeName)

    override fun handleElementRename(newElementName: String): PsiElement {
        val leaf = element.findElementAt(rangeInElement.startOffset) ?: return element
        leaf.replace(JuxElementFactory.createIdentifier(element.project, newElementName))
        return element
    }

    override fun getVariants(): Array<Any> = emptyArray()
}
