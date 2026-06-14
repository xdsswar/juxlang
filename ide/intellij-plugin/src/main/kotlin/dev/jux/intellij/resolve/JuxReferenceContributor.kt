package dev.jux.intellij.resolve

import com.intellij.openapi.util.TextRange
import com.intellij.patterns.PlatformPatterns
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiReference
import com.intellij.psi.PsiReferenceContributor
import com.intellij.psi.PsiReferenceProvider
import com.intellij.psi.PsiReferenceRegistrar
import com.intellij.psi.util.elementType
import com.intellij.util.ProcessingContext
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxCompositeElement
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Attaches a [JuxReference] to every node that names a *use* of something —
 * a reference expression, type reference, or member access.
 *
 * The reference lives on the **composite** node, not the identifier leaf:
 * provider-contributed references are only surfaced through
 * `ASTDelegatePsiElement.getReferences()` (which consults the provider
 * registry); plain leaves never ask the registry, so a leaf-targeted
 * provider silently contributes nothing. The reference's range narrows to
 * the *name* identifier inside the node (`obj.method` → `method`,
 * `a.b.Type<T>` → `Type`).
 */
class JuxReferenceContributor : PsiReferenceContributor() {
    override fun registerReferenceProviders(registrar: PsiReferenceRegistrar) {
        registrar.registerReferenceProvider(
            PlatformPatterns.psiElement(JuxCompositeElement::class.java),
            object : PsiReferenceProvider() {
                override fun getReferencesByElement(element: PsiElement, context: ProcessingContext): Array<PsiReference> {
                    if (element.elementType !in REFERENCE_PARENTS) return PsiReference.EMPTY_ARRAY
                    val name = nameLeaf(element) ?: return PsiReference.EMPTY_ARRAY
                    val range = TextRange.from(name.startOffsetInParent, name.textLength)
                    return arrayOf(JuxReference(element, range))
                }
            },
        )
    }

    /**
     * The identifier leaf the reference points at: the **last direct**
     * IDENTIFIER child — the simple name after any qualifier (`a.b.C` → `C`,
     * `obj.field` → `field`); generic arguments are nested nodes, so they
     * never shadow it.
     */
    private fun nameLeaf(element: PsiElement): PsiElement? {
        var last: PsiElement? = null
        var c: PsiElement? = element.firstChild
        while (c != null) {
            // A reserved keyword in member position (`recv.default`, `x.type`) is
            // the member name — accept it as the name leaf so keyword-named crate
            // members still get a reference (go-to / completion / highlight).
            if (c.elementType === JuxTokenTypes.IDENTIFIER ||
                JuxTokenTypes.KEYWORDS.contains(c.elementType)
            ) last = c
            c = c.nextSibling
        }
        return last
    }

    private companion object {
        val REFERENCE_PARENTS = setOf(
            E.REFERENCE_EXPRESSION,
            E.TYPE_REFERENCE,
            E.FIELD_ACCESS_EXPRESSION,
            E.METHOD_REF_EXPRESSION,
        )
    }
}
