package dev.jux.intellij.resolve

import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiReferenceBase
import com.intellij.psi.impl.source.resolve.ResolveCache
import dev.jux.intellij.highlight.JuxTokenTypes
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

    private companion object {
        val RESOLVER = ResolveCache.AbstractResolver<JuxReference, PsiElement> { ref, _ -> ref.doResolve() }
    }

    /**
     * Soft by design: this resolver only covers in-file symbols plus
     * project-wide types — an unresolved reference here is routinely a member
     * or std symbol the LSP owns, never an error the IDE should paint red.
     */
    override fun isSoft(): Boolean = true

    override fun resolve(): PsiElement? {
        // A bare parsing environment has no resolve cache; answer uncached there.
        val cache = element.project.getService(ResolveCache::class.java) ?: return doResolve()
        return cache.resolveWithCaching(this, RESOLVER, false, false)
    }

    private fun doResolve(): PsiElement? {
        val t = element.elementType
        // A call's arity picks between overloads of the same name.
        val parent = element.parent
        val argCount = if (parent?.elementType === E.CALL_EXPRESSION && parent.firstChild === element) {
            JuxTypeEngine.argumentCount(parent)
        } else {
            null
        }
        // The type engine first: it follows the receiver's real type through
        // chains, `var` inference, generics and bounds, inherited members and
        // imports -- the cases a name walk cannot see.
        // A name inside a type's `[...]` dimension is a value or a const
        // type parameter (`new int[N]`, `int[N] xs`), not the type.
        if (t === E.TYPE_REFERENCE && isInsideArrayDimension()) {
            return JuxTypeEngine.resolveReferenceExpression(element, null, value)
        }
        when (t) {
            E.FIELD_ACCESS_EXPRESSION ->
                JuxTypeEngine.resolveMemberAccess(element, argCount)?.let { return it.element }
            // `obj::greet`, `Type::staticMethod`, `Type::new` (§M.8): the
            // qualifier's type holds the member, exactly as for `obj.greet`.
            E.METHOD_REF_EXPRESSION -> {
                if (value == "new") {
                    JuxTypeEngine.constructorReferenceTarget(element)?.let { return it }
                } else {
                    JuxTypeEngine.resolveMemberAccess(element, null)?.let { return it.element }
                }
            }
            E.REFERENCE_EXPRESSION ->
                JuxTypeEngine.resolveReferenceExpression(element, argCount)?.let { return it }
            E.TYPE_REFERENCE -> {
                // `sealed enum Signal permits Red, Amber` (§7.7): the permitted
                // names are the enum's own variants.
                permittedEnumVariant()?.let { return it }
                val ids = element.node.getChildren(null)
                    .takeWhile { it.elementType !== E.TYPE_ARGUMENT_LIST && it.elementType !== JuxTokenTypes.LBRACKET }
                    .filter { it.elementType === JuxTokenTypes.IDENTIFIER }
                    .map { it.text }
                if (ids.isNotEmpty()) {
                    val qualifier = if (ids.size > 1) ids.dropLast(1).joinToString(".") else null
                    JuxTypeEngine.resolveTypeName(element, ids.last(), qualifier)?.let { return it }
                }
            }
        }
        // Member access (`recv.field` / `recv.method`): resolve through the
        // receiver's type FIRST, so Go-to lands on the right member even when an
        // unrelated enclosing-class member shares the name. Falls back to the
        // by-name walk when the receiver type can't be inferred in-file (stdlib
        // / chained receivers stay with the LSP).
        if (t === E.FIELD_ACCESS_EXPRESSION || t === E.METHOD_REF_EXPRESSION) {
            resolveMember()?.let { return it }
        }
        // A qualified type reference (`a.b.C`, `rust.std.PathBuf`) names a member
        // of a package, not an in-file/project symbol — resolving the bare last
        // segment would mis-jump to an unrelated top-level `C`. Defer to the LSP.
        if (t === E.TYPE_REFERENCE && element.text.substringBefore('<').contains('.')) {
            return null
        }
        return resolveLocally() ?: resolveCrossFile()
    }

    /** Whether this reference's name sits inside a type reference's `[...]`. */
    private fun isInsideArrayDimension(): Boolean {
        var c = element.node.firstChildNode
        var inside = false
        while (c != null) {
            if (c.startOffset - element.textRange.startOffset >= rangeInElement.startOffset) return inside
            when (c.elementType) {
                JuxTokenTypes.LBRACKET -> inside = true
                JuxTokenTypes.RBRACKET -> inside = false
            }
            c = c.treeNext
        }
        return false
    }

    /** The enum variant a name in an enum's own `permits` clause names, if any. */
    private fun permittedEnumVariant(): PsiElement? {
        if (element.parent?.elementType !== E.PERMITS_CLAUSE) return null
        val enumDecl = element.parent?.parent?.takeIf { it.elementType === E.ENUM_DECLARATION } ?: return null
        return com.intellij.psi.util.PsiTreeUtil.findChildrenOfType(enumDecl, JuxNamedElement::class.java)
            .firstOrNull { it.elementType === E.ENUM_CONSTANT && it.name == value }
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
        return JuxTypeIndex.findType(element, value)
    }

    private fun lookupInScope(scope: PsiElement, name: String, refOffset: Int): PsiElement? {
        when (scope.elementType) {
            E.CODE_BLOCK ->
                // Locals are visible only after their declaration in the block.
                for (child in dev.jux.intellij.psi.JuxLocals.blockLocals(scope)) {
                    if (child.elementType === E.LOCAL_VARIABLE && child.textOffset < refOffset &&
                        (child as? JuxNamedElement)?.name == name
                    ) return child
                }
            // The bindings a loop, a catch clause or a lambda introduces —
            // real declarations, so go-to-definition, rename and find-usages
            // all reach them.
            E.FOR_EACH_STATEMENT, E.FOR_STATEMENT, E.CATCH_CLAUSE ->
                scope.children.firstOrNull {
                    it.elementType === E.LOCAL_VARIABLE && (it as? JuxNamedElement)?.name == name
                }?.let { return it }
            E.LAMBDA_EXPRESSION -> {
                val list = scope.children.firstOrNull { it.elementType === E.PARAMETER_LIST }
                val params = (list?.children?.toList() ?: emptyList()) + scope.children
                params.firstOrNull {
                    it.elementType === E.PARAMETER && (it as? JuxNamedElement)?.name == name
                }?.let { return it }
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
