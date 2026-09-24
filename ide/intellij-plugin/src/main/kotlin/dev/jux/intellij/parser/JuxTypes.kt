package dev.jux.intellij.parser

import com.intellij.lang.PsiBuilder
import com.intellij.psi.tree.TokenSet
import com.intellij.psi.tree.IElementType
import dev.jux.intellij.highlight.JuxKeywords
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/*
 * Type, name, and generics parsing — shared by the declaration, statement, and
 * expression parsers. Top-level functions (same package) so every parser reads
 * them uniformly.
 */

/**
 * A dotted name `a.b.c`, wrapped in [E.QUALIFIED_NAME].
 *
 * A segment AFTER a `.` may be spelled with a keyword: the member-position
 * rule. `import rust.x.record;`, `package demo.type;`, `import rust.foo.drop;`
 * are all legal, because after the dot the grammar is unambiguous, so nothing
 * else could start there. Rust crates really do have modules called `type`,
 * `box` and `match`, and the compiler accepts them here; without this the IDE
 * red-flagged an import that compiles.
 *
 * Each keyword segment is REMAPPED to `IDENTIFIER` before it is consumed, the
 * same trick [consumeDeclName] uses, so the PSI, the reference contributor and
 * the annotator all see a plain name and need no special case. The FIRST
 * segment stays identifier-only: there a keyword would be a different
 * statement, not a name.
 *
 * The lookahead is what keeps `import a.b.*;`, `import a.b.{X, Y as Z};` and a
 * bare `package a.b;` intact: `*`, `{` and `;` are none of them names, so the
 * walk stops and the caller handles them.
 */
fun PsiBuilder.parseQualifiedName() {
    val m = mark()
    expectOrError(T.IDENTIFIER, "Identifier expected")
    while (at(T.DOT) && isPathSegmentToken(lookAhead(1))) {
        advanceLexer() // `.`
        if (!at(T.IDENTIFIER)) remapCurrentToken(T.IDENTIFIER)
        advanceLexer() // segment
    }
    m.done(E.QUALIFIED_NAME)
}

/** Can [t] spell a path segment after a `.`? An identifier, or any keyword. */
private fun isPathSegmentToken(t: IElementType?): Boolean =
    t === T.IDENTIFIER || (t != null && T.KEYWORDS.contains(t))

/**
 * A type reference: a named type (qualified name + optional generics), a
 * tuple/function type `( … ) -> T`, or `void`, followed by `[]` / `?` / `*`
 * suffixes.
 */
/** Tokens that can begin an operand: after `as T`, a `*` before one is multiplication. */
private val OPERAND_START: TokenSet = TokenSet.orSet(
    T.LITERALS,
    TokenSet.create(T.IDENTIFIER, T.LPAREN, T.MINUS, T.BANG, T.TILDE, T.THIS_KW, T.NEW_KW, T.SIZEOF_KW),
)

/**
 * The type after `as`: the same grammar, except that a `*` followed by an
 * operand ends the type (`n as long * 2` is a multiplication, §L.6.2).
 */
fun PsiBuilder.parseAsType() = parseType(asType = true)

fun PsiBuilder.parseType(asType: Boolean = false) {
    val m = mark()
    // Whether a base type was actually parsed. When it wasn't (the token here
    // can't start a type — e.g. a leading `*`/`&` of a deref/address-of
    // *expression* statement), we must NOT consume the trailing `* ? []`
    // suffixes: doing so would let a speculative local-variable parse swallow
    // `*p = value;` as a `type=*, name=p` decl and bury the rollback.
    var baseParsed = true
    when {
        at(T.LPAREN) -> {
            // Tuple `(A, B)` or function `(A) async throws T -> R` type.
            skipMatched(T.LPAREN, T.RPAREN)
            if (at(T.ASYNC_KW)) advanceLexer()
            if (at(T.THROWS_KW)) { advanceLexer(); parseTypeList() }
            if (at(T.ARROW)) { advanceLexer(); parseType() }
        }
        at(T.VOID_KW) -> advanceLexer()
        // Function-pointer type `fn(A, B) -> R` (Layout-ABI §L.6.4). `fn` is
        // contextual, exactly as in the compiler: only `fn` directly followed by
        // `(` in a type position starts one.
        at(T.IDENTIFIER) && tokenText == "fn" && lookAhead(1) === T.LPAREN -> {
            advanceLexer() // `fn`
            skipMatched(T.LPAREN, T.RPAREN)
            if (at(T.ARROW)) { advanceLexer(); parseType() }
        }
        at(T.IDENTIFIER) -> {
            advanceLexer()
            while (at(T.DOT) && lookAhead(1) === T.IDENTIFIER) {
                advanceLexer(); advanceLexer()
            }
            if (at(T.LT)) parseTypeArguments()
        }
        else -> {
            expectOrError(T.IDENTIFIER, "Type expected")
            baseParsed = false
        }
    }
    // Array / nullable / pointer suffixes — only after a real base type.
    if (baseParsed) {
        while (true) {
            when {
                // `T[]` and `T[N]`. Anything longer is not a type suffix (the
                // `[3;` of an unfinished `new int[3;` must not run to EOF).
                at(T.LBRACKET) && lookAhead(1) === T.RBRACKET -> { advanceLexer(); advanceLexer() }
                at(T.LBRACKET) && lookAhead(2) === T.RBRACKET -> { advanceLexer(); advanceLexer(); advanceLexer() }
                at(T.QUESTION) -> advanceLexer()
                // After `as`, `*` before an operand is multiplication:
                // `n as long * 2` (Layout-ABI §L.6.2).
                at(T.STAR) -> if (asType && lookAhead(1) in OPERAND_START) break else advanceLexer()
                else -> break
            }
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

fun PsiBuilder.parseTypeParameters() = parseAngleList(E.TYPE_PARAMETER_LIST, typeParams = true)

fun PsiBuilder.parseTypeArguments() = parseAngleList(E.TYPE_ARGUMENT_LIST, typeParams = false)

/**
 * Parse a `< … >` generic clause, exposing its meaningful pieces as PSI so they
 * navigate / resolve / highlight (find-usages, go-to-definition, rename):
 *
 *  - **Type arguments** (`List<? extends Animal>`, `Map<K, V>`): each depth-1
 *    type name becomes a [E.TYPE_REFERENCE]; a `?` (with optional
 *    `extends`/`super Bound`) becomes a [E.WILDCARD_TYPE] wrapping its bound.
 *  - **Type parameters** (`<T extends Animal & Speaks>`): the first name in each
 *    comma segment is the declared [E.TYPE_PARAMETER]; names after
 *    `extends`/`&` are bound [E.TYPE_REFERENCE]s.
 *
 * Depth is tracked exactly as [skipAngleBalanced] does — `<`/`<<` open one/two
 * levels, `>`/`>>`/`>>=` close one/two — so arbitrarily deep nesting
 * (`X<R<S<M<String>>>>` → `>>``>>`) balances correctly. Only depth-1 tokens are
 * wrapped; nested arguments (and the inside of `(…)`/`[…]` function/array types)
 * stay opaque, which keeps every marker well-formed across a shared `>>` close.
 * The scanner only *adds* markers over the same token walk, so it can never turn
 * a previously-accepted clause into a parse error.
 */
private fun PsiBuilder.parseAngleList(listType: IElementType, typeParams: Boolean) {
    val m = mark()
    if (!at(T.LT)) { m.done(listType); return }
    advanceLexer() // `<`
    var depth = 1
    // For type parameters, the first identifier of each comma segment is the
    // parameter NAME (a declaration); after `extends`/`&` come bound references.
    var atSegmentHead = typeParams
    // The previous depth-1 token was a complete type name, so an identifier
    // now means the `>` is missing (`Vec<int items`).
    var afterTypeName = false
    while (!eof() && depth > 0) {
        if (atAny(ANGLE_LIST_STOP) || (!typeParams && depth == 1 && afterTypeName && at(T.IDENTIFIER))) {
            errorHere("'>' expected")
            break
        }
        afterTypeName = false
        when (val t = tokenType) {
            T.LT -> { depth++; advanceLexer() }
            T.LT_LT -> { depth += 2; advanceLexer() }
            T.GT -> { depth--; advanceLexer() }
            T.GT_GT, T.GT_GT_EQ -> { depth -= 2; advanceLexer() }
            T.LPAREN -> skipMatched(T.LPAREN, T.RPAREN)     // function-type params
            T.LBRACKET -> skipMatched(T.LBRACKET, T.RBRACKET)
            else -> if (depth != 1) {
                advanceLexer() // nested level — opaque
            } else when {
                t === T.COMMA -> { atSegmentHead = typeParams; advanceLexer() }
                t === T.EXTENDS_KW || t === T.SUPER_KW -> { atSegmentHead = false; advanceLexer() }
                t === T.QUESTION && !typeParams -> {
                    val w = mark()
                    advanceLexer() // `?`
                    if (at(T.EXTENDS_KW) || at(T.SUPER_KW)) {
                        advanceLexer()
                        if (at(T.IDENTIFIER)) parseTypeRefName()
                    }
                    w.done(E.WILDCARD_TYPE)
                    atSegmentHead = false
                }
                t === T.IDENTIFIER && atSegmentHead && typeParams -> {
                    if (lookAhead(1) === T.IDENTIFIER && tokenText in JuxKeywords.PRIMITIVES) {
                        // Const generic `<int N>` (§A.2.6) — a primitive type
                        // then the declared parameter name. Gated on the
                        // primitive set exactly like the compiler
                        // (`is_known_primitive_type_name`), so an erroneous
                        // `<Foo Bar>` labels `Foo` as the parameter on both
                        // sides (rename/usages stay aligned in error states).
                        parseTypeRefName()
                        val p = mark(); advanceLexer(); p.done(E.TYPE_PARAMETER)
                    } else {
                        val p = mark(); advanceLexer(); p.done(E.TYPE_PARAMETER)
                    }
                    atSegmentHead = false
                }
                t === T.IDENTIFIER -> { parseTypeRefName(); atSegmentHead = false; afterTypeName = true }
                else -> advanceLexer()
            }
        }
    }
    m.done(listType)
}

/** Tokens that never appear inside a `< … >` clause: reaching one means the `>` is missing. */
private val ANGLE_LIST_STOP: TokenSet = TokenSet.create(T.SEMICOLON, T.LBRACE, T.RBRACE, T.EQ)

/** A qualified type NAME `a.b.C` (no generic suffix), wrapped in [E.TYPE_REFERENCE]. */
private fun PsiBuilder.parseTypeRefName() {
    val r = mark()
    advanceLexer() // identifier
    while (at(T.DOT) && lookAhead(1) === T.IDENTIFIER) { advanceLexer(); advanceLexer() }
    r.done(E.TYPE_REFERENCE)
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
        // A speculative probe must not run past a statement or a block.
        if (atAny(ANGLE_LIST_STOP)) return
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
