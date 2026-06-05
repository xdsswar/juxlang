package dev.jux.intellij.highlight

/**
 * Word sets the lexer uses to classify identifiers, kept faithful to the
 * compiler.
 *
 * [KEYWORDS] is the exact reserved-keyword set from `juxc-lex/src/token.rs`
 * (the `Keyword` enum) — the normative source. If a keyword is added there,
 * mirror it here. [PRIMITIVES] are the built-in type names (not reserved
 * keywords, but coloured as types for a Java-like feel). [CONSTANTS] are the
 * literal keywords.
 *
 * Annotations are handled by the lexer's `@`-prefix rule and are
 * case-insensitive (`@Override` ≡ `@override`), so they need no entry here.
 */
object JuxKeywords {
    /** Reserved keywords — verbatim from `juxc-lex` `Keyword` (55 entries). */
    val KEYWORDS: Set<String> = setOf(
        "abstract", "annotation", "as", "async", "await", "break", "case", "catch", "class",
        "const", "continue", "default", "do", "drop", "else", "enum", "extends", "final",
        "finally", "for", "if", "implements", "import", "init", "interface", "internal", "move",
        "native", "new", "operator", "package", "permits", "private", "protected", "public",
        "record", "return", "sealed", "sizeof", "static", "struct", "super", "switch", "this",
        "throw", "throws", "try", "type", "unsafe", "var", "void", "volatile", "when", "while",
        "yield",
    )

    /** Built-in type names — coloured as types (not reserved keywords). */
    val PRIMITIVES: Set<String> = setOf(
        "bool", "char", "byte", "short", "int", "long", "float", "double",
        "ubyte", "ushort", "uint", "ulong", "never", "String",
    )

    /** Literal constants (§A.2.9 — `constant.language`). */
    val CONSTANTS: Set<String> = setOf("true", "false", "null")
}
