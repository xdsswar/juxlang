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

/**
 * Statement/declaration terminator recovery: a missing `;` is non-fatal.
 *
 * A stray closer right before the `;` (`print(1));`) is the actual mistake, so
 * it is marked as such and the `;` still ends the statement: one error where
 * the old parse gave "';' expected" and then "unexpected token" on the same
 * character.
 */
fun PsiBuilder.semicolon() {
    if (expect(T.SEMICOLON)) return
    if ((at(T.RPAREN) || at(T.RBRACKET)) && lookAhead(1) === T.SEMICOLON) {
        val stray = mark()
        val text = tokenText
        advanceLexer()
        stray.error("Unexpected '$text'")
        advanceLexer() // `;`
        return
    }
    errorHere("';' expected")
}

/**
 * Parse a required expression, reporting "Expression expected" when none
 * starts here: `int x = ;`, `3 + ;`. Returns the expression marker, if any.
 */
fun PsiBuilder.parseExpressionOrError(): PsiBuilder.Marker? {
    val parsed = parseExpression()
    if (parsed == null) errorHere("Expression expected")
    return parsed
}

/**
 * The message for a token that cannot start a statement here, named for the
 * mistake it usually is: an `else` or `catch` whose opener is missing, a stray
 * closer, a `case` outside a switch.
 */
fun PsiBuilder.unexpectedTokenMessage(): String = when (tokenType) {
    T.ELSE_KW -> "'else' without 'if'"
    T.CATCH_KW -> "'catch' without 'try'"
    T.FINALLY_KW -> "'finally' without 'try'"
    T.CASE_KW -> "'case' outside a switch"
    else -> "Unexpected '${tokenText ?: ""}'"
}

/**
 * Modifiers that can only begin a member, never a statement. A block that
 * meets one is missing its closing brace: the member belongs to the enclosing
 * type, not to the method body.
 */
val MEMBER_ONLY_START: TokenSet = TokenSet.create(
    T.PUBLIC_KW, T.PRIVATE_KW, T.PROTECTED_KW, T.INTERNAL_KW, T.ABSTRACT_KW, T.SEALED_KW,
)

private val MISSING_BRACE_REPORTED_AT = com.intellij.openapi.util.Key.create<Int>("jux.missing.brace.offset")

/**
 * Close a `{ … }` block: consume `}`, or report "'}' expected" once per
 * position -- nested blocks that all end at the same member report it once.
 */
fun PsiBuilder.closeBrace() {
    if (expect(T.RBRACE)) return
    if (getUserData(MISSING_BRACE_REPORTED_AT) == currentOffset) return
    putUserData(MISSING_BRACE_REPORTED_AT, currentOffset)
    errorHere("'}' expected")
}

/**
 * Consume a **member name** — an identifier, or a reserved keyword used
 * contextually as one. After `.` / `?.` / `::` the grammar is unambiguous, so a
 * Rust crate member whose name collides with a Jux keyword (`recv.default()`,
 * `value.type()`) parses cleanly instead of erroring. Reports success.
 */
fun PsiBuilder.consumeMemberName(): Boolean {
    if (at(T.IDENTIFIER) || T.KEYWORDS.contains(tokenType)) {
        advanceLexer()
        return true
    }
    errorHere("Name expected")
    return false
}

/**
 * Keywords that can only OPEN a declaration or a statement. They are the one
 * thing a name slot must NOT swallow: a member left half-written
 * (`public int` then the next member) would otherwise consume `public` as the
 * missing name and cascade red through the rest of the type body.
 */
val NON_NAME_KEYWORDS: TokenSet = TokenSet.create(
    T.PUBLIC_KW, T.PRIVATE_KW, T.PROTECTED_KW, T.INTERNAL_KW,
    T.STATIC_KW, T.ABSTRACT_KW, T.FINAL_KW, T.SEALED_KW, T.CONST_KW,
    T.CLASS_KW, T.INTERFACE_KW, T.ENUM_KW, T.STRUCT_KW, T.ANNOTATION_KW,
    T.IMPORT_KW, T.PACKAGE_KW, T.EXTENDS_KW, T.IMPLEMENTS_KW, T.PERMITS_KW,
    T.THROWS_KW, T.OPERATOR_KW,
    T.RETURN_KW, T.IF_KW, T.ELSE_KW, T.FOR_KW, T.WHILE_KW, T.DO_KW,
    T.SWITCH_KW, T.TRY_KW, T.CATCH_KW, T.FINALLY_KW, T.THROW_KW,
    T.BREAK_KW, T.CONTINUE_KW, T.YIELD_KW,
    T.VAR_KW, T.VOID_KW, T.NEW_KW, T.THIS_KW, T.SUPER_KW,
)

/**
 * Consume a **declaration name** — an identifier, or a keyword used as one.
 *
 * Every caller is a name slot that follows a type or return type, so the
 * grammar is unambiguous there: a keyword can only be the declared name, never
 * the start of something else. `public void record() { … }` is a method called
 * `record`, and the compiler accepts it (`Parser::parse_decl_name`) precisely
 * because `record` is a keyword only where a declaration may begin. The IDE has
 * to agree, or a file that compiles reads as broken.
 *
 * The token is REMAPPED to `IDENTIFIER` before it is consumed, so the rest of
 * the plugin needs no special case: the PSI finds the name where it always
 * looks, and the annotator colors `record` as the method name it is rather
 * than as a keyword.
 */
fun PsiBuilder.consumeDeclName(message: String): Boolean {
    if (at(T.IDENTIFIER)) {
        advanceLexer()
        return true
    }
    val t = tokenType
    if (t != null && T.KEYWORDS.contains(t) && !NON_NAME_KEYWORDS.contains(t)) {
        remapCurrentToken(T.IDENTIFIER)
        advanceLexer()
        return true
    }
    errorHere(message)
    return false
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
