package dev.jux.intellij.psi

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
}
