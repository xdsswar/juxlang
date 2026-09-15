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
                    importReferences(element)?.let { return it }
                    if (element.elementType !in REFERENCE_PARENTS) return PsiReference.EMPTY_ARRAY
                    val name = nameLeaf(element) ?: return PsiReference.EMPTY_ARRAY
                    val range = TextRange.from(name.startOffsetInParent, name.textLength)
                    return arrayOf(JuxReference(element, range))
                }
            },
        )
    }

    /**
     * References from an import to the types it names, or null when [element]
     * is not part of one.
     *
     * `import some.Truck;` puts the path in a QUALIFIED_NAME whose last segment
     * is the type. A grouped `import some.{Auto, Bus as B};` leaves the items as
     * identifiers of the IMPORT_STATEMENT itself, after the package path; an
     * alias after `as` names nothing and gets no reference.
     */
    private fun importReferences(element: PsiElement): Array<PsiReference>? {
        val type = element.elementType
        if (type === E.QUALIFIED_NAME && element.parent?.elementType === E.IMPORT_STATEMENT) {
            // `import a.b.{...}` and `import a.b.*`: the path is only a package.
            var after = element.node.treeNext
            while (after != null && after.elementType === com.intellij.psi.TokenType.WHITE_SPACE) after = after.treeNext
            if (after != null && after.elementType === JuxTokenTypes.DOT) return PsiReference.EMPTY_ARRAY
            val ids = element.node.getChildren(null).filter { it.elementType === JuxTokenTypes.IDENTIFIER }
            if (ids.size < 2) return PsiReference.EMPTY_ARRAY
            val last = ids.last()
            val range = TextRange.from(last.startOffset - element.textRange.startOffset, last.textLength)
            val pkg = ids.dropLast(1).joinToString(".") { it.text }
            return arrayOf(JuxImportReference(element, range, pkg, last.text))
        }
        if (type === E.IMPORT_STATEMENT) {
            val path = element.node.findChildByType(E.QUALIFIED_NAME) ?: return PsiReference.EMPTY_ARRAY
            if (element.node.findChildByType(JuxTokenTypes.LBRACE) == null) return PsiReference.EMPTY_ARRAY
            val pkg = path.getChildren(null)
                .filter { it.elementType === JuxTokenTypes.IDENTIFIER }
                .joinToString(".") { it.text }
            val out = ArrayList<PsiReference>()
            var afterBrace = false
            var afterAs = false
            var c = element.node.firstChildNode
            while (c != null) {
                when (c.elementType) {
                    JuxTokenTypes.LBRACE -> afterBrace = true
                    JuxTokenTypes.AS_KW -> afterAs = true
                    JuxTokenTypes.COMMA -> afterAs = false
                    JuxTokenTypes.IDENTIFIER -> if (afterBrace && !afterAs) {
                        val range = TextRange.from(c.startOffset - element.textRange.startOffset, c.textLength)
                        out.add(JuxImportReference(element, range, pkg, c.text))
                    }
                }
                c = c.treeNext
            }
            return out.toTypedArray()
        }
        return null
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
