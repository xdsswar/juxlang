package dev.jux.intellij.resolve

import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiReferenceBase
import com.intellij.psi.search.PsiElementProcessor
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement

/**
 * A by-name reference from an identifier to an in-file declaration — the
 * IDE-side resolution the plugin owns (locals/params/fields/methods/types in
 * the same file). Cross-file and std/library symbols stay with `juxc-lsp`
 * (Rust std = Jux std), so an unresolved reference here is not an error — the
 * LSP annotates those.
 *
 * Resolution is a name match over the file's named declarations. Full lexical
 * scoping (shadowing, scope chains) is a later refinement; this already powers
 * Go-to-Declaration, Find Usages, and basic completion within a file.
 */
class JuxReference(element: PsiElement, range: TextRange) :
    PsiReferenceBase<PsiElement>(element, range) {

    /**
     * Soft by design: this resolver only covers in-file symbols plus
     * project-wide types — an unresolved reference here is routinely a member
     * or std symbol the LSP owns, never an error the IDE should paint red.
     */
    override fun isSoft(): Boolean = true

    override fun resolve(): PsiElement? = resolveLocally() ?: resolveCrossFile()

    /**
     * In-file resolution only — cheap (no index access), which is what the
     * semantic-highlighting annotator calls per identifier. Walks enclosing
     * scopes from innermost out; the first visible match wins (locals/params
     * shadow fields/types, inner blocks shadow outer).
     */
    fun resolveLocally(): PsiElement? {
        val name = value
        val refOffset = element.textOffset
        var scope: PsiElement? = element.parent
        while (scope != null) {
            lookupInScope(scope, name, refOffset)?.let { return it }
            scope = scope.parent
        }
        return null
    }

    /**
     * Cross-file fallback for **type positions** only: `extends Foo`,
     * `Foo x = …` — resolved through [JuxTypeIndex] (a project-wide scan, so
     * reserved for navigation; the annotator never reaches here). Member and
     * std symbols stay with the LSP.
     */
    private fun resolveCrossFile(): PsiElement? {
        if (element.elementType !== E.TYPE_REFERENCE) return null
        return JuxTypeIndex.findType(element.project, value)
    }

    private fun lookupInScope(scope: PsiElement, name: String, refOffset: Int): PsiElement? {
        when (scope.elementType) {
            E.CODE_BLOCK ->
                // Locals are visible only after their declaration in the block.
                for (child in scope.children) {
                    if (child.elementType === E.LOCAL_VARIABLE && child.textOffset < refOffset &&
                        (child as? JuxNamedElement)?.name == name
                    ) return child
                }
            E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                paramList(scope)?.let { list ->
                    for (p in list.children) {
                        if (p.elementType === E.PARAMETER && (p as? JuxNamedElement)?.name == name) return p
                    }
                }
            E.CLASS_BODY ->
                // Members are visible anywhere in the body, regardless of order.
                for (m in scope.children) {
                    if (m is JuxNamedElement && m.name == name) return m
                }
        }
        if (scope is JuxFile) {
            for (d in scope.children) {
                if (d is JuxNamedElement && d.name == name) return d
            }
        }
        return null
    }

    private fun paramList(method: PsiElement): PsiElement? =
        method.children.firstOrNull { it.elementType === E.PARAMETER_LIST }

    /**
     * Rename a usage: swap the **name leaf inside the range** for a fresh
     * identifier. The reference element is the whole composite node, so
     * replacing the element itself would erase the qualifier/arguments.
     */
    override fun handleElementRename(newElementName: String): PsiElement {
        val leaf = element.findElementAt(rangeInElement.startOffset) ?: return element
        leaf.replace(JuxElementFactory.createIdentifier(element.project, newElementName))
        return element
    }

    override fun getVariants(): Array<Any> {
        val file = element.containingFile ?: return emptyArray()
        val out = ArrayList<Any>()
        PsiTreeUtil.processElements(file, PsiElementProcessor { e ->
            if (e is JuxNamedElement) e.name?.let { out.add(LookupElementBuilder.create(it)) }
            true
        })
        return out.toTypedArray()
    }
}
