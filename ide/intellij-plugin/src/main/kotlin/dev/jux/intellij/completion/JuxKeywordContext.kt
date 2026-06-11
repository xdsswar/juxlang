package dev.jux.intellij.completion

import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxKeywords
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile

/**
 * Grammar-position keyword filtering for completion: offering `class` inside
 * a method body (or `return` at the top level) just buries the useful entries.
 * Sets are hand-curated against the spec's grammar, then **intersected with
 * the generated [JuxKeywords.KEYWORDS]** so a curation typo can never offer a
 * non-keyword (the generated list is single-sourced from `juxc-lex`).
 */
object JuxKeywordContext {
    private val MODIFIERS = curated(
        "public", "private", "protected", "internal", "static", "abstract",
        "final", "const", "sealed", "async", "unsafe", "volatile", "default",
        "weak", "native",
    )

    private val TYPE_DECLS = curated(
        "class", "interface", "enum", "record", "struct", "annotation", "type",
    )

    /** Supertype-clause keywords — type headers at file level AND nested. */
    private val HEADER_CLAUSES = curated("extends", "implements", "permits")

    /** File level: package/import headers, modifiers, type declarations. */
    val TOP_LEVEL: Set<String> =
        curated("package", "import", "void") + MODIFIERS + TYPE_DECLS + HEADER_CLAUSES

    /** Inside a class/interface/enum body: members, nested types, special blocks. */
    val MEMBER: Set<String> = curated(
        "void", "new", "operator", "init", "drop", "throws",
    ) + MODIFIERS + TYPE_DECLS + HEADER_CLAUSES

    /** Expression starters — valid anywhere an expression can begin. */
    val EXPRESSION: Set<String> = curated(
        "new", "this", "super", "true", "false", "null", "switch", "move",
        "await", "async", "sizeof",
    )

    /** Inside a code block: statement keywords + locals + expression starters. */
    val STATEMENT: Set<String> = curated(
        "if", "else", "while", "for", "do", "switch", "case", "default",
        "return", "throw", "try", "catch", "finally", "break", "continue",
        "var", "final", "const", "unsafe", "in", "as", "when", "yield",
    ) + EXPRESSION

    /**
     * The keyword set for the completion position [at] (the PSI leaf at the
     * caret): nearest enclosing scope wins — code block → statements, class
     * body → members, file → top level.
     */
    fun keywordsFor(at: PsiElement): Set<String> {
        var scope: PsiElement? = at
        while (scope != null && scope !is JuxFile) {
            when (scope.elementType) {
                E.CODE_BLOCK -> return STATEMENT
                E.CLASS_BODY -> return MEMBER
                // Inside any expression node but not yet inside a block —
                // e.g. a field initializer: offer expression starters.
                E.ARGUMENT_LIST, E.BINARY_EXPRESSION, E.ASSIGNMENT_EXPRESSION,
                E.PARENTHESIZED_EXPRESSION, E.CONDITIONAL_EXPRESSION,
                E.CALL_EXPRESSION, E.LAMBDA_EXPRESSION -> return EXPRESSION
                else -> {}
            }
            scope = scope.parent
        }
        return TOP_LEVEL
    }

    /** Curate against the generated keyword alphabet — typos can't leak through. */
    private fun curated(vararg words: String): Set<String> =
        words.filterTo(LinkedHashSet()) { it in JuxKeywords.KEYWORDS }
}
