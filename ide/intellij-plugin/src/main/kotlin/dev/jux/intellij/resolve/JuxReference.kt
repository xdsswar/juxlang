package dev.jux.intellij.resolve

import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiReferenceBase
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

    override fun resolve(): PsiElement? {
        // Member access (`recv.field` / `recv.method`): resolve through the
        // receiver's type FIRST, so Go-to lands on the right member even when an
        // unrelated enclosing-class member shares the name. Falls back to the
        // by-name walk when the receiver type can't be inferred in-file (stdlib
        // / chained receivers stay with the LSP).
        val t = element.elementType
        if (t === E.FIELD_ACCESS_EXPRESSION || t === E.METHOD_REF_EXPRESSION) {
            resolveMember()?.let { return it }
        }
        return resolveLocally() ?: resolveCrossFile()
    }

    /**
     * Resolve a `recv.name` member to its declaration: infer the receiver's type
     * with [JuxTypeInference] (this/super, a typed local/param/field, or a type
     * name for statics) and find the member named [value] among the type's
     * declared + inherited members ([JuxHierarchy.allMembers]). Only the common
     * single-identifier receiver is handled — a chained `a.b.name` would need
     * `b`'s type, which is the LSP's job — so it returns null there and the
     * caller falls back.
     */
    private fun resolveMember(): PsiElement? {
        val name = value
        val receiverWord = receiverWord() ?: return null
        val target = JuxTypeInference.resolveReceiver(receiverWord, element) ?: return null
        // Match static-ness to the receiver the same way member completion does
        // (`Type.x` → statics + enum constants; `obj.x` → instance members), so
        // a same-named static+instance pair resolves to the right one.
        return JuxHierarchy.allMembers(target.type).firstOrNull { m ->
            val named = m as? JuxNamedElement ?: return@firstOrNull false
            if (named.name != name) return@firstOrNull false
            val isStatic = m.elementType === E.ENUM_CONSTANT || JuxHierarchy.hasModifier(m, "static")
            target.isStatic == isStatic
        }
    }

    /**
     * The receiver identifier immediately left of the `.` before the member
     * name (the name leaf sits at [rangeInElement]). Returns null when there is
     * no qualifying `.` (a bare reference, not a member access) or the qualifier
     * isn't a single identifier.
     */
    private fun receiverWord(): String? {
        val text = element.text
        var i = rangeInElement.startOffset - 1
        while (i >= 0 && text[i].isWhitespace()) i--
        if (i < 0 || text[i] != '.') return null
        i--
        while (i >= 0 && text[i].isWhitespace()) i--
        val end = i + 1
        while (i >= 0 && (text[i].isLetterOrDigit() || text[i] == '_')) i--
        val start = i + 1
        if (end <= start) return null
        // Defer chained receivers (`a.b.name`): the receiver `b` is itself
        // qualified, so resolving it as a bare in-scope value/type would be
        // wrong (its type is `a`'s member type, which is the LSP's job). Only a
        // single-identifier receiver (`recv.name`, `this.name`) is handled.
        while (i >= 0 && text[i].isWhitespace()) i--
        if (i >= 0 && text[i] == '.') return null
        return text.substring(start, end)
    }

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
                // §L.7 C-FFI: a `native { … }` block's foreign functions are
                // file-level callables, so a bare `lstrlenA(…)` resolves into it.
                if (d.elementType === E.EXTERN_BLOCK) {
                    for (fn in d.children) {
                        if (fn is JuxNamedElement && fn.name == name) return fn
                    }
                }
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

    /**
     * No reference-driven variants: the platform would surface these for ANY
     * reference at the caret — including member positions after `.` — flooding
     * the lookup with every name in the file regardless of scope. Fallback
     * completion is owned by [dev.jux.intellij.completion.JuxCompletionContributor]
     * (scope-aware, relevance-ranked); member completion by `juxc-lsp`.
     */
    override fun getVariants(): Array<Any> = emptyArray()
}
