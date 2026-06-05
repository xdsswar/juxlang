package dev.jux.intellij

import com.intellij.lang.Language

/**
 * The Jux language singleton.
 *
 * Phase 1 registers no `ParserDefinition` — Jux is a "syntax-highlight-only"
 * language whose coloring comes from the bundled TextMate grammar and whose
 * semantics come from `juxc-lsp`. This is a supported configuration (Markdown
 * and several bundled languages do the same). PSI lands later (§I.8).
 */
object JuxLanguage : Language("Jux") {
    private fun readResolve(): Any = JuxLanguage
}
