package dev.jux.intellij.parser

import com.intellij.lang.PsiBuilder
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/*
 * Type, name, and generics parsing — shared by the declaration, statement, and
 * expression parsers. Top-level functions (same package) so every parser reads
 * them uniformly.
 */

/** A dotted name `a.b.c`, wrapped in [E.QUALIFIED_NAME]. */
fun PsiBuilder.parseQualifiedName() {
    val m = mark()
    expectOrError(T.IDENTIFIER, "identifier expected")
    while (at(T.DOT) && lookAhead(1) === T.IDENTIFIER) {
        advanceLexer() // `.`
        advanceLexer() // ident
    }
    m.done(E.QUALIFIED_NAME)
}

/**
 * A type reference: a named type (qualified name + optional generics), a
 * tuple/function type `( … ) -> T`, or `void`, followed by `[]` / `?` / `*`
 * suffixes.
 */
fun PsiBuilder.parseType() {
    val m = mark()
    when {
        at(T.LPAREN) -> {
            // Tuple `(A, B)` or function `(A) async throws T -> R` type.
            skipMatched(T.LPAREN, T.RPAREN)
            if (at(T.ASYNC_KW)) advanceLexer()
            if (at(T.THROWS_KW)) { advanceLexer(); parseTypeList() }
            if (at(T.ARROW)) { advanceLexer(); parseType() }
        }
        at(T.VOID_KW) -> advanceLexer()
        else -> {
            expectOrError(T.IDENTIFIER, "type expected")
            while (at(T.DOT) && lookAhead(1) === T.IDENTIFIER) {
                advanceLexer(); advanceLexer()
            }
            if (at(T.LT)) parseTypeArguments()
        }
    }
    // Array / nullable / pointer suffixes.
    while (true) {
        when {
            at(T.LBRACKET) -> skipMatched(T.LBRACKET, T.RBRACKET)
            at(T.QUESTION) -> advanceLexer()
            at(T.STAR) -> advanceLexer()
            else -> break
        }
    }
    m.done(E.TYPE_REFERENCE)
}

/** Comma-separated type list (for extends/implements/permits/throws). */
fun PsiBuilder.parseTypeList() {
    parseType()
    while (at(T.COMMA)) {
        advanceLexer()
        parseType()
    }
}

fun PsiBuilder.parseTypeParameters() {
    val m = mark()
    skipAngleBalanced()
    m.done(E.TYPE_PARAMETER_LIST)
}

fun PsiBuilder.parseTypeArguments() {
    val m = mark()
    skipAngleBalanced()
    m.done(E.TYPE_ARGUMENT_LIST)
}

/**
 * Consume a balanced `< … >` run, accounting for `>>`/`>>=`-style tokens (which
 * close two levels) so nested generics `Map<K, List<V>>` parse even though the
 * lexer emits a single `>>`.
 */
fun PsiBuilder.skipAngleBalanced() {
    if (!at(T.LT)) return
    var depth = 0
    while (!eof()) {
        when (tokenType) {
            T.LT -> depth++
            T.LT_LT -> depth += 2
            T.GT -> depth--
            T.GT_GT -> depth -= 2
            T.GT_GT_EQ -> depth -= 2
        }
        advanceLexer()
        if (depth <= 0) return
    }
}
