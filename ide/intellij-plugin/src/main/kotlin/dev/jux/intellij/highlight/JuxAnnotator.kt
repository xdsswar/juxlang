package dev.jux.intellij.highlight

import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.Annotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxObservableProps
import dev.jux.intellij.resolve.JuxReference

/**
 * PSI-based semantic highlighting — the layer the flat lexer can't provide.
 * Colours, off the parse tree:
 *
 * - **primitive type names** (`int`, `i32`, `String`, …) — identifiers at the
 *   lexer level, recognised here by position (inside a type reference) + the
 *   compiler-shared [JuxKeywords.PRIMITIVES] set;
 * - **annotation names** — the identifier(s) after `@`;
 * - **declaration names** — the class/interface/enum, method, and field name
 *   identifiers, each in its own colour (Java-like "the name stands out");
 * - **reference uses** (decl-vs-use colouring): call sites, parameter/local
 *   reads, field reads, type-parameter uses, enum constants, and class-name
 *   references — classified by position (call shape, type position) plus the
 *   **in-file** resolver ([JuxReference.resolveLocally]). Cross-file resolve
 *   is deliberately never invoked from here: it scans the whole project, and
 *   the annotator runs per identifier on every editor pass.
 */
class JuxAnnotator : Annotator {
    override fun annotate(element: PsiElement, holder: AnnotationHolder) {
        if (element.elementType !== JuxTokenTypes.IDENTIFIER) return
        val key = colorFor(element) ?: return
        holder.newSilentAnnotation(HighlightSeverity.INFORMATION)
            .range(element)
            .textAttributes(key)
            .create()
    }

    private fun colorFor(id: PsiElement): TextAttributesKey? {
        val parent = id.parent ?: return null

        // §P.5: `observer` is a reserved primitive keyword, colored anywhere —
        // same color as `int` / `bool` / `void`. juxc still lexes it as a
        // contextual identifier (no OBSERVER_KW in jux-tokens.json), so the
        // annotator owns this coloring rather than the lexer.
        if (id.text == JuxObservableProps.OBSERVER_TYPE) return JuxSyntaxHighlighter.TYPE

        // Declaration name: the name identifier of a class/method/field/etc.
        val named = parent as? JuxNamedElement
        if (named != null && named.nameIdentifier === id) {
            return when (parent.elementType) {
                E.CLASS_DECLARATION, E.INTERFACE_DECLARATION, E.ENUM_DECLARATION,
                E.RECORD_DECLARATION, E.STRUCT_DECLARATION, E.ANNOTATION_DECLARATION,
                E.TYPE_ALIAS_DECLARATION -> JuxSyntaxHighlighter.CLASS_NAME
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                    JuxSyntaxHighlighter.METHOD_DECLARATION
                E.FIELD_DECLARATION, E.CONST_DECLARATION, E.PROPERTY_DECLARATION ->
                    JuxSyntaxHighlighter.FIELD
                E.ENUM_CONSTANT -> JuxSyntaxHighlighter.ENUM_CONSTANT
                E.PARAMETER -> JuxSyntaxHighlighter.PARAMETER
                E.LOCAL_VARIABLE -> JuxSyntaxHighlighter.LOCAL_VARIABLE
                else -> null
            }
        }

        // Annotation name: `@Name` / `@a.b.C`.
        if (parent.elementType === E.QUALIFIED_NAME && parent.parent?.elementType === E.ANNOTATION) {
            return JuxSyntaxHighlighter.ANNOTATION
        }

        // Type position: primitive names, type-parameter uses, class names.
        if (parent.elementType === E.TYPE_REFERENCE) {
            val name = id.text
            if (name in JuxKeywords.PRIMITIVES) return JuxSyntaxHighlighter.TYPE
            if (isEnclosingTypeParameter(id, name)) return JuxSyntaxHighlighter.TYPE_PARAMETER
            // A capitalized name in type position is a class reference whether
            // or not it resolves in-file — cross-file/std types look right
            // without paying for an index scan.
            if (name.firstOrNull()?.isUpperCase() == true) return JuxSyntaxHighlighter.CLASS_NAME
            return null
        }

        // Type-parameter *declarations* (`class Box<T>` — the `T`): the parser
        // wraps them in TYPE_PARAMETER nodes, which aren't named elements.
        if (parent.elementType === E.TYPE_PARAMETER) {
            return JuxSyntaxHighlighter.TYPE_PARAMETER
        }

        // Reference uses: classify by call shape first (works without resolve,
        // so cross-file calls colour correctly too), then by in-file resolve.
        if (parent.elementType in REFERENCE_PARENTS) {
            // §P.5 native coloring must run BEFORE the call-shape check —
            // `attach(…)` is a call, and METHOD_CALL would win otherwise.
            propertyContextColor(id, parent)?.let { return it }

            if (isCallPosition(id, parent)) return JuxSyntaxHighlighter.METHOD_CALL

            // The reference lives on the composite node, ranged over its name
            // leaf — only colour when that leaf is THIS identifier (so a
            // qualifier never borrows the member's resolution).
            val ref = parent.references.firstOrNull() as? JuxReference ?: return null
            if (ref.rangeInElement.startOffset != id.startOffsetInParent) return null
            val resolved = ref.resolveLocally()
            if (resolved == null) {
                // `value` inside a setter body (§P.1.4) is the implicit
                // parameter — colored as one when nothing in scope shadows it.
                if (id.text == JuxObservableProps.SETTER_VALUE && isInSetterBody(id)) {
                    return JuxSyntaxHighlighter.PARAMETER
                }
                return null
            }
            return when (resolved.elementType) {
                E.CLASS_DECLARATION, E.INTERFACE_DECLARATION, E.ENUM_DECLARATION,
                E.RECORD_DECLARATION, E.STRUCT_DECLARATION, E.ANNOTATION_DECLARATION,
                E.TYPE_ALIAS_DECLARATION -> JuxSyntaxHighlighter.CLASS_NAME
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                    JuxSyntaxHighlighter.METHOD_CALL
                E.FIELD_DECLARATION, E.CONST_DECLARATION, E.PROPERTY_DECLARATION ->
                    JuxSyntaxHighlighter.FIELD
                E.ENUM_CONSTANT -> JuxSyntaxHighlighter.ENUM_CONSTANT
                E.PARAMETER -> JuxSyntaxHighlighter.PARAMETER
                E.LOCAL_VARIABLE -> JuxSyntaxHighlighter.LOCAL_VARIABLE
                else -> null
            }
        }

        return null
    }

    // ---- §P.5 native coloring (observable properties) ----------------------

    /**
     * Context-sensitive native coloring (§P.5 / §P.7.7): `observers` after a
     * property, `attach`/`detach`/`clear`/`size` after `.observers`, and
     * `bind`/`unbind`/`bindBidirectional` directly after a property. Used
     * anywhere else, these names are plain identifiers and return null here.
     *
     * Name-gated so every other identifier pays nothing. In-file resolve
     * VETOES (a local named `bind` or a user method `attach` stays plain);
     * receivers that don't resolve in-file fall back to documented heuristics —
     * `.observers` chains color optimistically, the bind family only behind a
     * PascalCase receiver. The full type-resolved pass (§P.7.7) is jux-ls
     * semantic-tokens territory; this is the IDE-side approximation.
     */
    private fun propertyContextColor(id: PsiElement, parent: PsiElement): TextAttributesKey? {
        val name = id.text
        // Cheap gate: bail unless the identifier is one of the magic names.
        val isMember = name == JuxObservableProps.OBSERVERS_MEMBER
        val isOp = name in JuxObservableProps.OBSERVERS_OPS
        val isBind = name in JuxObservableProps.BIND_OPS
        if (!isMember && !isOp && !isBind) return null

        // All §P shapes are member accesses: `<recv>.<name>`.
        if (parent.elementType !== E.FIELD_ACCESS_EXPRESSION) return null
        if (!isLastIdentifier(id, parent)) return null
        val recv = parent.firstChild ?: return null

        return when {
            // `<property>.observers`
            isMember ->
                if (isPropertyReceiver(recv, optimistic = true)) JuxSyntaxHighlighter.NATIVE_MEMBER
                else null
            // `<property>.observers.attach(…)` / `.detach(…)` (calls) and
            // `.clear` / `.size` (paren-free command accessors, §P.3.2 — with
            // parens they lose the color, a useful smell).
            isOp -> {
                if (!isObserversChain(recv)) return null
                val wantsCall = name !in JuxObservableProps.PAREN_FREE_OPS
                if (isCallee(parent) == wantsCall) JuxSyntaxHighlighter.NATIVE_OPERATION else null
            }
            // `<property>.bind(…)` / `.unbind()` / `.bindBidirectional(…)`
            else ->
                if (isCallee(parent) && isPropertyReceiver(recv, optimistic = false))
                    JuxSyntaxHighlighter.NATIVE_OPERATION
                else null
        }
    }

    /** True when [recv] is a `….observers` access on a qualifying property. */
    private fun isObserversChain(recv: PsiElement): Boolean {
        if (recv.elementType !== E.FIELD_ACCESS_EXPRESSION) return false
        val last = lastIdentifier(recv) ?: return false
        if (last.text != JuxObservableProps.OBSERVERS_MEMBER) return false
        val inner = recv.firstChild ?: return false
        return isPropertyReceiver(inner, optimistic = true)
    }

    /**
     * Does [recv] denote an observable property? In-file resolution decides
     * when it can ([JuxReference.resolveLocally] — never cross-file from the
     * annotator); an unresolved receiver is accepted [optimistic]ally (the
     * `.observers` chains) or behind the PascalCase convention (bind family).
     */
    private fun isPropertyReceiver(recv: PsiElement, optimistic: Boolean): Boolean {
        if (recv.elementType !in REFERENCE_PARENTS) return false
        val ref = recv.references.firstOrNull() as? JuxReference
        val resolved = ref?.resolveLocally()
        if (resolved != null) return resolved.elementType === E.PROPERTY_DECLARATION
        if (optimistic) return true
        val last = lastIdentifier(recv) ?: return false
        return last.text.firstOrNull()?.isUpperCase() == true
    }

    /** True when [parent] is the callee of a `CALL_EXPRESSION`. */
    private fun isCallee(parent: PsiElement): Boolean {
        val grand = parent.parent ?: return false
        return grand.elementType === E.CALL_EXPRESSION && grand.firstChild === parent
    }

    /** True when [id] sits inside a `set { … }` accessor body (§P.1.4). */
    private fun isInSetterBody(id: PsiElement): Boolean {
        var scope: PsiElement? = id.parent
        while (scope != null) {
            if (scope.elementType === E.PROPERTY_ACCESSOR) {
                return scope.firstIdentifierText() == "set"
            }
            // A method/class boundary means we left any accessor body.
            if (scope.elementType === E.METHOD_DECLARATION ||
                scope.elementType === E.CLASS_BODY
            ) return false
            scope = scope.parent
        }
        return false
    }

    /**
     * True when [id] is the *called name* of a call: the callee of a
     * `CALL_EXPRESSION` (`foo(…)`) or the member name of a field access that
     * is itself being called (`obj.method(…)` — the access node is the
     * call's callee and [id] is its last identifier).
     */
    private fun isCallPosition(id: PsiElement, parent: PsiElement): Boolean {
        val grand = parent.parent ?: return false
        if (grand.elementType !== E.CALL_EXPRESSION) return false
        // The callee is the call's first child; an identifier inside an
        // ARGUMENT_LIST must not be classified as the called name.
        if (grand.firstChild !== parent) return false
        return when (parent.elementType) {
            E.REFERENCE_EXPRESSION -> true
            // `obj.method(…)` — only the trailing name is the method.
            E.FIELD_ACCESS_EXPRESSION, E.METHOD_REF_EXPRESSION -> isLastIdentifier(id, parent)
            else -> false
        }
    }

    /** True when [id] is the last IDENTIFIER leaf directly under [parent]. */
    private fun isLastIdentifier(id: PsiElement, parent: PsiElement): Boolean =
        lastIdentifier(parent) === id

    /** The last IDENTIFIER leaf directly under [parent], or null. */
    private fun lastIdentifier(parent: PsiElement): PsiElement? {
        var last: PsiElement? = null
        var c: PsiElement? = parent.firstChild
        while (c != null) {
            if (c.elementType === JuxTokenTypes.IDENTIFIER) last = c
            c = c.nextSibling
        }
        return last
    }

    /**
     * True when [name] matches a type parameter declared by an enclosing
     * method or type declaration (`class Box<T>` / `<T> T id(T x)`).
     */
    private fun isEnclosingTypeParameter(at: PsiElement, name: String): Boolean {
        var scope: PsiElement? = at.parent
        while (scope != null) {
            val t = scope.elementType
            if (t === E.CLASS_DECLARATION || t === E.INTERFACE_DECLARATION ||
                t === E.ENUM_DECLARATION || t === E.RECORD_DECLARATION ||
                t === E.STRUCT_DECLARATION || t === E.METHOD_DECLARATION ||
                t === E.TYPE_ALIAS_DECLARATION
            ) {
                val params = scope.node.findChildByType(E.TYPE_PARAMETER_LIST)?.psi
                if (params != null) {
                    var p: PsiElement? = params.firstChild
                    while (p != null) {
                        if (p.elementType === E.TYPE_PARAMETER &&
                            p.firstIdentifierText() == name
                        ) return true
                        p = p.nextSibling
                    }
                }
            }
            scope = scope.parent
        }
        return false
    }

    private fun PsiElement.firstIdentifierText(): String? {
        var c: PsiElement? = firstChild
        while (c != null) {
            if (c.elementType === JuxTokenTypes.IDENTIFIER) return c.text
            c = c.nextSibling
        }
        return null
    }

    private companion object {
        val REFERENCE_PARENTS = setOf(
            E.REFERENCE_EXPRESSION,
            E.FIELD_ACCESS_EXPRESSION,
            E.METHOD_REF_EXPRESSION,
        )
    }
}
