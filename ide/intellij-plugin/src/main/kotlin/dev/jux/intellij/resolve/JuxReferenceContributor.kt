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
                    labelReference(element)?.let { return arrayOf(it) }
                    // `a * b` / `a..b` / `v *= k`: the operator token names the
                    // user `operator` it calls (§O.2.3).
                    JuxOperatorReference.of(element)?.let { return arrayOf(it) }
                    if (element.elementType !in REFERENCE_PARENTS) return PsiReference.EMPTY_ARRAY
                    val name = nameLeaf(element) ?: return PsiReference.EMPTY_ARRAY
                    // `x.operator hash()` (§O.2.7) names an operator every value
                    // has, not a member called `hash`.
                    if (isNamedOperatorName(name)) return PsiReference.EMPTY_ARRAY
                    val range = TextRange.from(name.startOffsetInParent, name.textLength)
                    val main = JuxReference(element, range)
                    // `int[N]` / `new Cell[size]`: a name inside an array
                    // dimension is a value (or a const type parameter), with a
                    // reference of its own beside the type's.
                    val dims = arrayDimensionNames(element)
                    if (dims.isEmpty()) return arrayOf(main)
                    return (listOf<PsiReference>(main) + dims.map {
                        JuxReference(element, TextRange.from(it.startOffsetInParent, it.textLength))
                    }).toTypedArray()
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
     * `break outer;` / `continue outer;`: a reference from the label to the
     * labeled statement it leaves (Grammar §A.2.8), so go-to, find usages and
     * rename treat a label like any other name.
     */
    private fun labelReference(element: PsiElement): PsiReference? {
        if (element.elementType !== E.BREAK_STATEMENT && element.elementType !== E.CONTINUE_STATEMENT) return null
        val label = element.node.findChildByType(JuxTokenTypes.IDENTIFIER)?.psi ?: return null
        return JuxLabelReference(element, TextRange.from(label.startOffsetInParent, label.textLength))
    }

    /** The identifiers inside a type reference's `[...]` dimensions. */
    private fun arrayDimensionNames(element: PsiElement): List<PsiElement> {
        if (element.elementType !== E.TYPE_REFERENCE) return emptyList()
        val out = ArrayList<PsiElement>()
        var inside = false
        var c: PsiElement? = element.firstChild
        while (c != null) {
            when (c.elementType) {
                JuxTokenTypes.LBRACKET -> inside = true
                JuxTokenTypes.RBRACKET -> inside = false
                JuxTokenTypes.IDENTIFIER -> if (inside) out.add(c)
            }
            c = c.nextSibling
        }
        return out
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
            // A type's name comes before its `[...]` dimensions.
            if (c.elementType === JuxTokenTypes.LBRACKET && element.elementType === E.TYPE_REFERENCE) break
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

    /** True when [name] is the `hash` of `x.operator hash()`. */
    private fun isNamedOperatorName(name: PsiElement): Boolean {
        var prev = name.prevSibling
        while (prev != null && prev.elementType === com.intellij.psi.TokenType.WHITE_SPACE) prev = prev.prevSibling
        return prev?.elementType === JuxTokenTypes.OPERATOR_KW
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
