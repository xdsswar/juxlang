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
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/**
 * Attaches a [JuxReference] to every identifier that names a *use* of something
 * (a reference/type/member-access name) — but not to a declaration's own name,
 * which is the target, not a reference.
 */
class JuxReferenceContributor : PsiReferenceContributor() {
    override fun registerReferenceProviders(registrar: PsiReferenceRegistrar) {
        registrar.registerReferenceProvider(
            PlatformPatterns.psiElement(JuxTokenTypes.IDENTIFIER),
            object : PsiReferenceProvider() {
                override fun getReferencesByElement(element: PsiElement, context: ProcessingContext): Array<PsiReference> {
                    val parent = element.parent ?: return PsiReference.EMPTY_ARRAY
                    if (parent.elementType !in REFERENCE_PARENTS) return PsiReference.EMPTY_ARRAY
                    // A declaration's own name identifier is a definition, not a reference.
                    if (parent is JuxNamedElement && parent.nameIdentifier === element) return PsiReference.EMPTY_ARRAY
                    return arrayOf(JuxReference(element, TextRange(0, element.textLength)))
                }
            },
        )
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
