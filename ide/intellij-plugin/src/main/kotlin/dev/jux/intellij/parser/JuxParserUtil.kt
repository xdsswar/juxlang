package dev.jux.intellij.parser

import com.intellij.lang.PsiBuilder
import com.intellij.psi.tree.IElementType
import com.intellij.psi.tree.TokenSet
import dev.jux.intellij.highlight.JuxTokenTypes as T

/*
 * Thin helpers over [PsiBuilder] mirroring IntelliJ's `JavaParserUtil` idiom:
 * a marker-based recursive-descent surface with non-throwing error recovery.
 * Whitespace and comments are skipped automatically by the builder (they are
 * in the parser definition's whitespace/comment token sets), so these helpers
 * only ever see significant tokens.
 *
 * Declared as top-level extensions (same package as the parser) so they read as
 * `b.expect(...)` without ceremony.
 */

/** True if the current token is [type]. */
fun PsiBuilder.at(type: IElementType): Boolean = tokenType === type

/** True if the current token is in [set]. */
fun PsiBuilder.atAny(set: TokenSet): Boolean = set.contains(tokenType)

/** Consume the current token if it is [type]; report whether it was. */
fun PsiBuilder.expect(type: IElementType): Boolean {
    if (at(type)) {
        advanceLexer()
        return true
    }
    return false
}

/** Consume [type] or emit a zero-width error; report success. */
fun PsiBuilder.expectOrError(type: IElementType, message: String): Boolean {
    if (expect(type)) return true
    errorHere(message)
    return false
}

/** Statement/declaration terminator recovery: a missing `;` is non-fatal. */
fun PsiBuilder.semicolon() {
    expectOrError(T.SEMICOLON, "';' expected")
}

/** Emit a zero-width error node at the current position without consuming. */
fun PsiBuilder.errorHere(message: String) {
    val m = mark()
    m.error(message)
}

/**
 * Consume a run of tokens with balanced `()`/`[]`/`{}` until a token in [stops]
 * is reached at depth 0 (or EOF). Used to swallow expression and initializer
 * text the declaration-level parser does not yet descend into — the basis the
 * full expression parser (Phase 3) replaces.
 */
fun PsiBuilder.consumeBalancedUntil(stops: TokenSet) {
    var depth = 0
    while (!eof()) {
        val t = tokenType
        if (depth == 0 && stops.contains(t)) return
        when (t) {
            T.LPAREN, T.LBRACKET, T.LBRACE -> depth++
            T.RPAREN, T.RBRACKET, T.RBRACE -> if (depth == 0) return else depth--
        }
        advanceLexer()
    }
}

/**
 * Consume a `[open] … [close]` run, tracking nesting so inner pairs don't end
 * it early. Leaves the cursor just past the matching [close]. No-op if the
 * cursor isn't on [open].
 */
fun PsiBuilder.skipMatched(open: IElementType, close: IElementType) {
    if (!at(open)) return
    var depth = 0
    while (!eof()) {
        val t = tokenType
        if (t === open) depth++
        else if (t === close) {
            depth--
            if (depth == 0) { advanceLexer(); return }
        }
        advanceLexer()
    }
}

/**
 * `ref` is being reserved compiler-side (parallel work): `public ref String x`
 * declares a reference to an object rather than a copy. Looked up by NAME so
 * this compiles before `jux-tokens.json` regenerates — null today; once the
 * generated registry gains REF_KW it joins the modifier set (and the lexer
 * colors it as a keyword) with zero edits here. Same pattern as `typeof`.
 */
val JUX_REF_KW: IElementType? = T.keywordType("ref")

/** True when the current token is the (post-landing) `ref` keyword. */
fun PsiBuilder.atRefKw(): Boolean = JUX_REF_KW != null && tokenType === JUX_REF_KW

/** Keyword modifiers that may prefix a declaration. */
val JUX_MODIFIERS: TokenSet = TokenSet.orSet(
    TokenSet.create(
        T.PUBLIC_KW, T.PRIVATE_KW, T.PROTECTED_KW, T.INTERNAL_KW,
        T.STATIC_KW, T.ABSTRACT_KW, T.FINAL_KW, T.CONST_KW, T.SEALED_KW,
        T.ASYNC_KW, T.NATIVE_KW, T.UNSAFE_KW, T.VOLATILE_KW, T.DEFAULT_KW,
        T.WEAK_KW, // `weak` field modifier (§6.5)
    ),
    JUX_REF_KW?.let { TokenSet.create(it) } ?: TokenSet.EMPTY,
)

/** Keywords that open a type declaration. */
val JUX_TYPE_DECL_KEYWORDS: TokenSet = TokenSet.create(
    T.CLASS_KW, T.INTERFACE_KW, T.ENUM_KW, T.RECORD_KW, T.STRUCT_KW, T.ANNOTATION_KW,
)
