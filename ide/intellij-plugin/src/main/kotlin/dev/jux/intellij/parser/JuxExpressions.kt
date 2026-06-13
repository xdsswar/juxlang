package dev.jux.intellij.parser

import com.intellij.lang.PsiBuilder
import com.intellij.psi.tree.IElementType
import com.intellij.psi.tree.TokenSet
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/*
 * Expression parser — a precedence cascade mirroring the canonical Rust parser
 * (`crates/juxc-parse/src/exprs.rs`) and the §A.4 operator table, built on the
 * platform marker API. Each `parseXxx` returns the completed [PsiBuilder.Marker]
 * of the expression it parsed (so a caller can `.precede()` it to build a
 * left-deepening tree), or null if no expression starts here.
 *
 * Precedence, loosest to tightest: assignment, ternary `?:`, elvis `?:`/`??`,
 * `||`, `&&`, `|`, `^`, `&`, equality, comparison, `<=>`, type-test `=>`,
 * range `..`, shift, additive, multiplicative, `as` cast, prefix unary,
 * postfix (`. ?. [] () :: ? !!`), primary. Bitwise binds looser than equality
 * (Java/Jux order), per §A.4.
 */

/**
 * `typeof` is being reserved compiler-side (parallel work): looked up by NAME
 * so this file compiles before `jux-tokens.json` regenerates. Null today; the
 * moment the generated [T] registry gains TYPEOF_KW, lexing/coloring/parsing
 * all light up with zero edits here (the lexer classifies keywords through the
 * same `keywordType` lookup). Once the token lands for good, these nullable
 * references can be replaced by direct `T.TYPEOF_KW` — purely cosmetic.
 */
private val TYPEOF_KW: IElementType? = T.keywordType("typeof")

private val TYPEOF_SET: TokenSet =
    TYPEOF_KW?.let { TokenSet.create(it) } ?: TokenSet.EMPTY

/** Tokens that can begin an expression — used for `?` and statement lookahead. */
val JUX_EXPR_START: TokenSet = TokenSet.orSet(
    T.LITERALS,
    TokenSet.create(
        T.IDENTIFIER, T.THIS_KW, T.SUPER_KW, T.NEW_KW, T.SWITCH_KW, T.IF_KW, T.TRY_KW,
        T.SIZEOF_KW,
        T.LPAREN, T.LBRACKET,
        T.BANG, T.MINUS, T.PLUS, T.TILDE, T.AMP, T.STAR, T.MOVE_KW, T.AWAIT_KW, T.ASYNC_KW,
    ),
    TYPEOF_SET,
)

private val ASSIGN_OPS: TokenSet = TokenSet.create(
    T.EQ, T.PLUS_EQ, T.MINUS_EQ, T.STAR_EQ, T.SLASH_EQ, T.PERCENT_EQ,
    T.AMP_EQ, T.PIPE_EQ, T.CARET_EQ, T.LT_LT_EQ, T.GT_GT_EQ,
)
private val ELVIS_OPS: TokenSet = TokenSet.create(T.QUESTION_COLON, T.QUESTION_QUESTION)
private val PREFIX_OPS: TokenSet = TokenSet.create(
    T.BANG, T.MINUS, T.PLUS, T.TILDE, T.AMP, T.STAR, T.MOVE_KW, T.AWAIT_KW,
)

/** Tokens after `(Type)` that make it a cast rather than a parenthesized expr. */
private val CAST_FOLLOW: TokenSet = TokenSet.orSet(
    T.LITERALS,
    TokenSet.create(
        T.IDENTIFIER, T.THIS_KW, T.SUPER_KW, T.NEW_KW, T.LPAREN, T.SIZEOF_KW,
        T.BANG, T.TILDE, T.MOVE_KW, T.AWAIT_KW,
    ),
    TYPEOF_SET,
)

/** Entry point: parse a full expression (including lambdas and assignment). */
fun PsiBuilder.parseExpression(): PsiBuilder.Marker? {
    tryParseLambda()?.let { return it }
    return parseAssignment()
}

private fun PsiBuilder.parseAssignment(): PsiBuilder.Marker? {
    val left = parseTernary() ?: return null
    if (atAny(ASSIGN_OPS)) {
        val m = left.precede()
        advanceLexer()
        parseExpression() // right-associative; allows a lambda on the RHS
        m.done(E.ASSIGNMENT_EXPRESSION)
        return m
    }
    return left
}

private fun PsiBuilder.parseTernary(): PsiBuilder.Marker? {
    val cond = parseElvis() ?: return null
    if (at(T.QUESTION)) {
        val m = cond.precede()
        advanceLexer() // `?`
        parseExpression() // then-branch
        expectOrError(T.COLON, "':' expected in ternary")
        parseTernary() // else-branch (right-associative)
        m.done(E.CONDITIONAL_EXPRESSION)
        return m
    }
    return cond
}

private fun PsiBuilder.parseElvis(): PsiBuilder.Marker? {
    val left = parseBinary(1) ?: return null
    if (atAny(ELVIS_OPS)) {
        val m = left.precede()
        advanceLexer()
        parseElvis() // right-associative
        m.done(E.BINARY_EXPRESSION)
        return m
    }
    return left
}

/** Precedence-climbing core for the binary/range/type-test/cast layers. */
private fun PsiBuilder.parseBinary(minPrec: Int): PsiBuilder.Marker? {
    var left = parseUnary() ?: return null
    while (true) {
        val op = tokenType
        val prec = binaryPrecedence(op)
        if (prec < 0 || prec < minPrec) break
        val m = left.precede()
        advanceLexer() // operator
        val node = when (op) {
            T.AS_KW -> { parseType(); E.CAST_EXPRESSION }
            T.FAT_ARROW -> { // type-test `e => Type [binding]`
                parseType()
                if (at(T.IDENTIFIER)) advanceLexer()
                E.BINARY_EXPRESSION
            }
            T.DOT_DOT, T.DOT_DOT_EQ -> {
                parseBinary(prec + 1)
                if (atContextual("step")) { advanceLexer(); parseBinary(prec + 1) }
                E.RANGE_EXPRESSION
            }
            else -> { parseBinary(prec + 1); E.BINARY_EXPRESSION }
        }
        m.done(node)
        left = m
    }
    return left
}

private fun binaryPrecedence(op: IElementType?): Int = when (op) {
    T.OR_OR -> 1
    T.AND_AND -> 2
    T.PIPE -> 3
    T.CARET -> 4
    T.AMP -> 5
    T.EQ_EQ, T.NOT_EQ, T.STRICT_EQ, T.STRICT_NOT_EQ -> 6
    T.LT, T.LE, T.GT, T.GE -> 7
    T.SPACESHIP -> 8
    T.FAT_ARROW -> 9
    T.DOT_DOT, T.DOT_DOT_EQ -> 10
    // Wrapping variants bind exactly like their ordinary counterparts (§A.4).
    T.LT_LT, T.GT_GT, T.LT_LT_PERCENT, T.GT_GT_PERCENT -> 11
    T.PLUS, T.MINUS, T.PLUS_PERCENT, T.MINUS_PERCENT -> 12
    T.STAR, T.SLASH, T.PERCENT, T.STAR_PERCENT -> 13
    T.AS_KW -> 14
    else -> -1
}

private fun PsiBuilder.parseUnary(): PsiBuilder.Marker? {
    if (atAny(PREFIX_OPS)) {
        val m = mark()
        advanceLexer()
        parseUnary()
        m.done(E.UNARY_EXPRESSION)
        return m
    }
    parseCast()?.let { return it }
    return parsePostfix()
}

/** Speculative C-style cast `(Type) expr`; rolls back to a parenthesized expr. */
private fun PsiBuilder.parseCast(): PsiBuilder.Marker? {
    if (!at(T.LPAREN)) return null
    val m = mark()
    advanceLexer() // `(`
    parseType()
    if (at(T.RPAREN) && CAST_FOLLOW.contains(lookAhead(1))) {
        advanceLexer() // `)`
        parseUnary()
        m.done(E.CAST_EXPRESSION)
        return m
    }
    m.rollbackTo()
    return null
}

private fun PsiBuilder.parsePostfix(): PsiBuilder.Marker? {
    var operand = parsePrimary() ?: return null
    while (true) {
        when (tokenType) {
            T.LPAREN -> {
                val m = operand.precede()
                parseArgumentList()
                m.done(E.CALL_EXPRESSION)
                operand = m
            }
            T.LBRACKET -> {
                val m = operand.precede()
                advanceLexer() // `[`
                parseExpression()
                expectOrError(T.RBRACKET, "']' expected")
                m.done(E.INDEX_EXPRESSION)
                operand = m
            }
            T.DOT -> {
                val m = operand.precede()
                advanceLexer() // `.`
                if (at(T.INT_LITERAL)) advanceLexer() else expectOrError(T.IDENTIFIER, "name expected")
                m.done(E.FIELD_ACCESS_EXPRESSION)
                operand = m
            }
            T.QUESTION_DOT -> {
                val m = operand.precede()
                advanceLexer() // `?.`
                expectOrError(T.IDENTIFIER, "name expected")
                m.done(E.FIELD_ACCESS_EXPRESSION)
                operand = m
            }
            T.COLON_COLON -> {
                val m = operand.precede()
                advanceLexer() // `::`
                if (!expect(T.NEW_KW)) expectOrError(T.IDENTIFIER, "member name expected")
                m.done(E.METHOD_REF_EXPRESSION)
                operand = m
            }
            T.LT -> {
                // Explicit type arguments before a call: `foo<T>(...)`. Only
                // commit if a `(` follows the `<…>`; otherwise it's less-than.
                val m = operand.precede()
                val probe = mark()
                skipAngleBalanced()
                if (at(T.LPAREN)) {
                    probe.drop()
                    parseArgumentList()
                    m.done(E.CALL_EXPRESSION)
                    operand = m
                } else {
                    probe.rollbackTo()
                    m.drop()
                    break
                }
            }
            T.QUESTION -> {
                // Postfix error-propagation `e?`, but only when `?` is not the
                // start of a ternary (i.e. the next token can't begin an expr).
                if (JUX_EXPR_START.contains(lookAhead(1))) break
                val m = operand.precede()
                advanceLexer()
                m.done(E.POSTFIX_EXPRESSION)
                operand = m
            }
            T.BANG_BANG -> {
                val m = operand.precede()
                advanceLexer()
                m.done(E.POSTFIX_EXPRESSION)
                operand = m
            }
            else -> break
        }
    }
    return operand
}

private fun PsiBuilder.parsePrimary(): PsiBuilder.Marker? {
    val t = tokenType
    return when {
        T.LITERALS.contains(t) -> single(E.LITERAL_EXPRESSION)
        t === T.IDENTIFIER -> single(E.REFERENCE_EXPRESSION)
        t === T.THIS_KW -> single(E.THIS_EXPRESSION)
        t === T.SUPER_KW -> single(E.SUPER_EXPRESSION)
        t === T.NEW_KW -> parseNew()
        t === T.SIZEOF_KW -> parseSizeof()
        TYPEOF_KW != null && t === TYPEOF_KW -> parseTypeof()
        t === T.SWITCH_KW -> parseSwitchExpression()
        t === T.IF_KW -> parseIfExpression()
        t === T.TRY_KW -> parseTryExpression() // `var v = try { … } catch (…) { … };`
        t === T.LPAREN -> parseParenthesized()
        t === T.LBRACKET -> parseArrayLiteral()
        t === T.LBRACE -> parseBraceInitializer()
        else -> null
    }
}

/** A brace-enclosed aggregate/array initializer: `{ a, b, c }`. */
private fun PsiBuilder.parseBraceInitializer(): PsiBuilder.Marker {
    val m = mark()
    advanceLexer() // `{`
    if (!at(T.RBRACE)) {
        parseExpression()
        while (at(T.COMMA)) { advanceLexer(); if (!at(T.RBRACE)) parseExpression() }
    }
    expectOrError(T.RBRACE, "'}' expected")
    m.done(E.LITERAL_EXPRESSION)
    return m
}

/** `sizeof(Type)` (or `sizeof(expr)`) — §A.4 unary level. */
private fun PsiBuilder.parseSizeof(): PsiBuilder.Marker {
    val m = mark()
    advanceLexer() // `sizeof`
    if (expect(T.LPAREN)) {
        parseType()
        expectOrError(T.RPAREN, "')' expected")
    } else {
        parseUnary()
    }
    m.done(E.UNARY_EXPRESSION)
    return m
}

/**
 * `typeof '(' expression ')'` (§5.9.10) — the compile-time static-type-name
 * query, mirroring `juxc-parse/src/exprs.rs`. Unlike `sizeof` the operand is a
 * full EXPRESSION (`typeof(i + 1)`), and the parentheses are mandatory.
 */
private fun PsiBuilder.parseTypeof(): PsiBuilder.Marker {
    val m = mark()
    advanceLexer() // `typeof`
    expectOrError(T.LPAREN, "'(' expected after 'typeof'")
    parseExpression()
    expectOrError(T.RPAREN, "')' expected to close 'typeof'")
    m.done(E.UNARY_EXPRESSION)
    return m
}

private fun PsiBuilder.single(type: IElementType): PsiBuilder.Marker {
    val m = mark()
    advanceLexer()
    m.done(type)
    return m
}

private fun PsiBuilder.parseParenthesized(): PsiBuilder.Marker {
    val m = mark()
    advanceLexer() // `(`
    if (!at(T.RPAREN)) {
        parseExpression()
        while (at(T.COMMA)) { advanceLexer(); parseExpression() } // tuple
    }
    expectOrError(T.RPAREN, "')' expected")
    m.done(E.PARENTHESIZED_EXPRESSION)
    return m
}

private fun PsiBuilder.parseArrayLiteral(): PsiBuilder.Marker {
    val m = mark()
    advanceLexer() // `[`
    if (!at(T.RBRACKET)) {
        parseExpression()
        while (at(T.COMMA)) { advanceLexer(); if (!at(T.RBRACKET)) parseExpression() }
    }
    expectOrError(T.RBRACKET, "']' expected")
    m.done(E.LITERAL_EXPRESSION)
    return m
}

private fun PsiBuilder.parseNew(): PsiBuilder.Marker {
    val m = mark()
    advanceLexer() // `new`
    parseType()
    when {
        at(T.LPAREN) -> {
            parseArgumentList()
            // Anonymous class body `new T() { … }` (opaque for now).
            if (at(T.LBRACE)) skipMatched(T.LBRACE, T.RBRACE)
        }
        at(T.LBRACKET) -> {
            skipMatched(T.LBRACKET, T.RBRACKET)
            if (at(T.LBRACE)) skipMatched(T.LBRACE, T.RBRACE) // array initializer
        }
        at(T.LBRACE) -> skipMatched(T.LBRACE, T.RBRACE)
    }
    m.done(E.NEW_EXPRESSION)
    return m
}

private fun PsiBuilder.parseIfExpression(): PsiBuilder.Marker {
    val m = mark()
    advanceLexer() // `if`
    if (expect(T.LPAREN)) { parseExpression(); expectOrError(T.RPAREN, "')' expected") }
    parseExpression() // then
    if (expect(T.ELSE_KW)) parseExpression() // else (required for if-expr, lenient)
    m.done(E.CONDITIONAL_EXPRESSION)
    return m
}

private fun PsiBuilder.parseArgumentList() {
    val m = mark()
    advanceLexer() // `(`
    if (!at(T.RPAREN)) {
        parseArgument()
        while (at(T.COMMA)) { advanceLexer(); parseArgument() }
    }
    expectOrError(T.RPAREN, "')' expected")
    m.done(E.ARGUMENT_LIST)
}

private fun PsiBuilder.parseArgument() {
    // Named argument `name: expr`, or `out`/`move` prefixed, or plain expr.
    if (at(T.IDENTIFIER) && lookAhead(1) === T.COLON) {
        advanceLexer() // name
        advanceLexer() // `:`
    }
    // Out-arg mode (§6.x): `tryParse("42", out n)` — `out` is contextual, so
    // it only counts when an identifier follows.
    if (atContextual("out") && lookAhead(1) === T.IDENTIFIER) advanceLexer()
    parseExpression()
}

/**
 * Lambda heads: `x -> …`, `(…) -> …`, optionally `async`-prefixed. Speculative:
 * scans the parameter shape and rolls back if no `->` follows, so an ordinary
 * parenthesized expression is unaffected.
 */
private fun PsiBuilder.tryParseLambda(): PsiBuilder.Marker? {
    val m = mark()
    if (at(T.ASYNC_KW) && (lookAhead(1) === T.IDENTIFIER || lookAhead(1) === T.LPAREN)) advanceLexer()
    val ok = when {
        at(T.IDENTIFIER) && lookAhead(1) === T.ARROW -> { advanceLexer(); advanceLexer(); parseLambdaBody(); true }
        at(T.LPAREN) -> {
            skipMatched(T.LPAREN, T.RPAREN)
            if (at(T.ARROW)) { advanceLexer(); parseLambdaBody(); true } else false
        }
        else -> false
    }
    return if (ok) {
        m.done(E.LAMBDA_EXPRESSION)
        m
    } else {
        m.rollbackTo()
        null
    }
}

private fun PsiBuilder.parseLambdaBody() {
    if (at(T.LBRACE)) parseBlock() else parseExpression()
}

private fun PsiBuilder.atContextual(text: String): Boolean =
    at(T.IDENTIFIER) && tokenText == text
