package dev.jux.intellij.psi

import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes

/**
 * Shared vocabulary of the observable-property surface (§P,
 * `JUX-OBSERVABLE-PROPERTIES-ADDENDUM.md`). None of these are lexer keywords —
 * `observer`, `get`, `set`, `value`, `observers`, the operation names, and the
 * binding names all lex as plain IDENTIFIER tokens (mirroring `juxc-lex`, whose
 * generated `jux-tokens.json` this plugin must not diverge from). Everything
 * §P-shaped is therefore recognized by token *text* in context: the parser for
 * accessor blocks, the annotator for native coloring, the inspections and the
 * gutter provider for attach/bind site detection.
 */
object JuxObservableProps {
    /** `observer<T>` — the observer primitive type name (§P.2). */
    const val OBSERVER_TYPE = "observer"

    /** Accessor kinds inside a `{ … }` property block (§P.1 — `init` was removed). */
    val ACCESSOR_KINDS = setOf("get", "set")

    /** Operations on the `.observers` member (§P.3.2). */
    val OBSERVERS_OPS = setOf("attach", "detach", "clear", "size")

    /** `.observers` ops written WITHOUT parentheses — property-like command accessors. */
    val PAREN_FREE_OPS = setOf("clear", "size")

    /** Binding operations called directly on a property (§P.4). */
    val BIND_OPS = setOf("bind", "unbind", "bindBidirectional")

    /** The `.observers` member name itself (§P.3.1 — native-colored, not reserved). */
    const val OBSERVERS_MEMBER = "observers"

    /** The implicit setter parameter (§P.1.4 — contextual, C# convention). */
    const val SETTER_VALUE = "value"

    /**
     * True when [element] sits inside a `set { … }` accessor body, where
     * [SETTER_VALUE] is bound.
     *
     * `value` is contextual: inside a setter it is the implicit parameter
     * holding the value being assigned, and everywhere else — including inside
     * a GETTER, which the compiler rejects with E0301 — it is an ordinary
     * identifier. Every feature that treats it specially has to agree on that
     * boundary, or the editor colors it as a parameter while the inspection
     * calls it unresolved. So the boundary lives here, once.
     */
    fun isInSetterBody(element: PsiElement): Boolean {
        var scope: PsiElement? = element.parent
        while (scope != null) {
            if (scope.elementType === JuxElementTypes.PROPERTY_ACCESSOR) {
                return firstIdentifierText(scope) in setOf("set")
            }
            // A method or class boundary means we left any accessor body.
            if (scope.elementType === JuxElementTypes.METHOD_DECLARATION ||
                scope.elementType === JuxElementTypes.CLASS_BODY
            ) {
                return false
            }
            scope = scope.parent
        }
        return false
    }

    /** The text of [scope]'s first direct IDENTIFIER child — an accessor's kind. */
    private fun firstIdentifierText(scope: PsiElement): String? {
        var c: PsiElement? = scope.firstChild
        while (c != null) {
            if (c.elementType === JuxTokenTypes.IDENTIFIER) return c.text
            c = c.nextSibling
        }
        return null
    }
}
