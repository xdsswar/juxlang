package dev.jux.intellij.parser

import com.intellij.lang.PsiBuilder
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/*
 * Statement parser — code blocks and every statement form, mirroring the Rust
 * `stmts.rs` plus the spec's fuller set (do-while, C-style for, labeled, unsafe,
 * switch-as-statement). The classic local-variable-vs-expression ambiguity
 * (`Foo bar;` vs `foo.bar`) is resolved by a speculative type+name parse that
 * rolls back to an expression on failure.
 */

/** A `{ … }` block of statements. */
fun PsiBuilder.parseBlock() {
    val m = mark()
    expectOrError(T.LBRACE, "'{' expected")
    while (!eof() && !at(T.RBRACE)) {
        val before = currentOffset
        parseStatement()
        if (currentOffset == before) { // no progress — skip a token to recover
            val e = mark()
            advanceLexer()
            e.error("unexpected token")
        }
    }
    expectOrError(T.RBRACE, "'}' expected")
    m.done(E.CODE_BLOCK)
}

fun PsiBuilder.parseStatement() {
    when (tokenType) {
        T.LBRACE -> parseBlock()
        T.IF_KW -> parseIfStatement()
        T.WHILE_KW -> parseWhileStatement()
        T.DO_KW -> parseDoWhileStatement()
        T.FOR_KW -> parseForStatement()
        T.SWITCH_KW -> { parseSwitch(asExpression = false) }
        T.RETURN_KW -> parseSimple(E.RETURN_STATEMENT, hasOptionalExpr = true)
        T.THROW_KW -> parseSimple(E.THROW_STATEMENT, hasOptionalExpr = false, requireExpr = true)
        T.BREAK_KW -> parseBreakContinue(E.BREAK_STATEMENT)
        T.CONTINUE_KW -> parseBreakContinue(E.CONTINUE_STATEMENT)
        T.TRY_KW -> parseTryStatement()
        T.UNSAFE_KW -> { val m = mark(); advanceLexer(); parseBlock(); m.done(E.UNSAFE_STATEMENT) }
        T.SEMICOLON -> { val m = mark(); advanceLexer(); m.done(E.EMPTY_STATEMENT) }
        T.VAR_KW, T.FINAL_KW, T.CONST_KW -> parseLocalVariable()
        else -> parseLabeledOrExprOrLocal()
    }
}

private fun PsiBuilder.parseSimple(type: com.intellij.psi.tree.IElementType, hasOptionalExpr: Boolean, requireExpr: Boolean = false) {
    val m = mark()
    advanceLexer() // keyword
    if (requireExpr || (hasOptionalExpr && !at(T.SEMICOLON))) parseExpression()
    semicolon()
    m.done(type)
}

private fun PsiBuilder.parseBreakContinue(type: com.intellij.psi.tree.IElementType) {
    val m = mark()
    advanceLexer() // break / continue
    if (at(T.IDENTIFIER)) advanceLexer() // optional label
    semicolon()
    m.done(type)
}

private fun PsiBuilder.parseIfStatement() {
    val m = mark()
    advanceLexer() // `if`
    if (atContextualKw("cfg")) advanceLexer() // compile-time `if cfg(...)`
    if (expect(T.LPAREN)) {
        parseExpression()
        expectOrError(T.RPAREN, "')' expected")
    }
    parseStatement()
    if (expect(T.ELSE_KW)) parseStatement()
    m.done(E.IF_STATEMENT)
}

private fun PsiBuilder.parseWhileStatement() {
    val m = mark()
    advanceLexer() // `while`
    if (expect(T.LPAREN)) {
        parseExpression()
        expectOrError(T.RPAREN, "')' expected")
    }
    parseStatement()
    m.done(E.WHILE_STATEMENT)
}

private fun PsiBuilder.parseDoWhileStatement() {
    val m = mark()
    advanceLexer() // `do`
    parseStatement()
    expectOrError(T.WHILE_KW, "'while' expected")
    if (expect(T.LPAREN)) {
        parseExpression()
        expectOrError(T.RPAREN, "')' expected")
    }
    semicolon()
    m.done(E.DO_WHILE_STATEMENT)
}

private fun PsiBuilder.parseForStatement() {
    val m = mark()
    advanceLexer() // `for`
    if (at(T.AWAIT_KW)) advanceLexer() // `for await`
    expectOrError(T.LPAREN, "'(' expected")

    // for-each: `(var|Type) name : iterable`
    val probe = mark()
    if (tryForEachHeader() && at(T.COLON)) {
        probe.drop()
        advanceLexer() // `:`
        parseExpression()
        expectOrError(T.RPAREN, "')' expected")
        parseStatement()
        m.done(E.FOR_EACH_STATEMENT)
        return
    }
    probe.rollbackTo()

    // C-style: init? ; cond? ; update?
    if (!at(T.SEMICOLON)) parseForInit()
    expectOrError(T.SEMICOLON, "';' expected")
    if (!at(T.SEMICOLON)) parseExpression()
    expectOrError(T.SEMICOLON, "';' expected")
    if (!at(T.RPAREN)) {
        parseExpression()
        while (at(T.COMMA)) { advanceLexer(); parseExpression() }
    }
    expectOrError(T.RPAREN, "')' expected")
    parseStatement()
    m.done(E.FOR_STATEMENT)
}

private fun PsiBuilder.tryForEachHeader(): Boolean {
    while (at(T.FINAL_KW) || at(T.CONST_KW)) advanceLexer()
    if (at(T.VAR_KW)) advanceLexer() else parseType()
    if (!at(T.IDENTIFIER)) return false
    advanceLexer() // name
    return true
}

private fun PsiBuilder.parseForInit() {
    if (at(T.VAR_KW) || at(T.FINAL_KW) || at(T.CONST_KW)) {
        while (at(T.FINAL_KW) || at(T.CONST_KW)) advanceLexer()
        if (at(T.VAR_KW)) advanceLexer() else parseType()
        if (at(T.IDENTIFIER)) advanceLexer()
        if (expect(T.EQ)) parseExpression()
        return
    }
    // Typed init `int i = 0` (speculative) or expression list.
    val p = mark()
    parseType()
    if (at(T.IDENTIFIER)) {
        advanceLexer()
        if (expect(T.EQ)) parseExpression()
        p.drop()
        return
    }
    p.rollbackTo()
    parseExpression()
    while (at(T.COMMA)) { advanceLexer(); parseExpression() }
}

private fun PsiBuilder.parseLocalVariable() {
    val m = mark()
    while (at(T.FINAL_KW) || at(T.CONST_KW)) advanceLexer()
    if (at(T.VAR_KW)) advanceLexer() else parseType()
    if (at(T.LPAREN)) skipMatched(T.LPAREN, T.RPAREN) // destructuring `var (x, y)`
    else expectOrError(T.IDENTIFIER, "variable name expected")
    if (expect(T.EQ)) parseExpression()
    semicolon()
    m.done(E.LOCAL_VARIABLE)
}

private fun PsiBuilder.parseLabeledOrExprOrLocal() {
    // Labeled statement: `name:` followed by a loop/block.
    if (at(T.IDENTIFIER) && lookAhead(1) === T.COLON) {
        val m = mark()
        advanceLexer() // label
        advanceLexer() // `:`
        parseStatement()
        m.done(E.LABELED_STATEMENT)
        return
    }
    // Local var with explicit type (`Foo bar = …;`) vs expression statement.
    val m = mark()
    if (tryLocalVarTail()) {
        m.done(E.LOCAL_VARIABLE)
    } else {
        m.rollbackTo()
        val e = mark()
        parseExpression()
        semicolon()
        e.done(E.EXPRESSION_STATEMENT)
    }
}

/** Parse `Type name [= expr];`; returns false (for rollback) if it isn't one. */
private fun PsiBuilder.tryLocalVarTail(): Boolean {
    parseType()
    if (!at(T.IDENTIFIER)) return false
    advanceLexer() // name
    if (expect(T.EQ)) parseExpression()
    semicolon()
    return true
}

private fun PsiBuilder.parseTryStatement() {
    parseTryCore()
}

/** `try { … } catch (…) { … }` in **expression** position — used by [parsePrimary]. */
fun PsiBuilder.parseTryExpression(): PsiBuilder.Marker = parseTryCore()

private fun PsiBuilder.parseTryCore(): PsiBuilder.Marker {
    val m = mark()
    advanceLexer() // `try`
    parseBlock()
    while (at(T.CATCH_KW)) {
        val cm = mark()
        advanceLexer()
        if (expect(T.LPAREN)) {
            // Multi-catch: `catch (NetError | TimeoutError e)`.
            parseType()
            while (at(T.PIPE)) { advanceLexer(); parseType() }
            expectOrError(T.IDENTIFIER, "exception name expected")
            expectOrError(T.RPAREN, "')' expected")
        }
        parseBlock()
        cm.done(E.CATCH_CLAUSE)
    }
    if (at(T.FINALLY_KW)) {
        val fm = mark()
        advanceLexer()
        parseBlock()
        fm.done(E.FINALLY_CLAUSE)
    }
    m.done(E.TRY_STATEMENT)
    return m
}

// ---- switch (shared by statement and expression positions) ----------------

/** Switch as an expression — used by [parsePrimary]. */
fun PsiBuilder.parseSwitchExpression(): PsiBuilder.Marker = parseSwitch(asExpression = true)

fun PsiBuilder.parseSwitch(asExpression: Boolean): PsiBuilder.Marker {
    val m = mark()
    advanceLexer() // `switch`
    if (expect(T.LPAREN)) {
        parseExpression()
        expectOrError(T.RPAREN, "')' expected")
    }
    expectOrError(T.LBRACE, "'{' expected")
    while (!eof() && !at(T.RBRACE)) {
        val before = currentOffset
        parseSwitchCase()
        if (currentOffset == before) { val e = mark(); advanceLexer(); e.error("unexpected token") }
    }
    expectOrError(T.RBRACE, "'}' expected")
    m.done(if (asExpression) E.SWITCH_EXPRESSION else E.SWITCH_STATEMENT)
    return m
}

private fun PsiBuilder.parseSwitchCase() {
    val m = mark()
    when {
        at(T.CASE_KW) -> {
            advanceLexer()
            parsePattern()
            // `,`-separated case lists and `|` or-patterns (`case A | B ->`).
            while (at(T.COMMA) || at(T.PIPE)) { advanceLexer(); parsePattern() }
            if (at(T.WHEN_KW)) { val g = mark(); advanceLexer(); parseExpression(); g.done(E.PATTERN_GUARD) }
        }
        at(T.DEFAULT_KW) -> advanceLexer()
        else -> { m.drop(); return }
    }
    // `->` (lenient: also accept `=>`) then an expression `;` or a block.
    if (!expect(T.ARROW)) expect(T.FAT_ARROW)
    if (at(T.LBRACE)) parseBlock() else { parseExpression(); semicolon() }
    m.done(E.SWITCH_CASE)
}

/**
 * A lenient pattern: optional `var`/`final`, then a literal (with optional
 * range), or a qualified name optionally followed by a nested-pattern list
 * `(…)` or a binding identifier.
 */
private fun PsiBuilder.parsePattern() {
    val m = mark()
    while (at(T.FINAL_KW) || at(T.CONST_KW)) advanceLexer()
    if (at(T.VAR_KW)) advanceLexer()
    when {
        T.LITERALS.contains(tokenType) -> {
            advanceLexer()
            if (at(T.DOT_DOT) || at(T.DOT_DOT_EQ)) {
                advanceLexer()
                if (T.LITERALS.contains(tokenType)) advanceLexer()
            }
        }
        at(T.IDENTIFIER) -> {
            // qualified name
            advanceLexer()
            while (at(T.DOT) && lookAhead(1) === T.IDENTIFIER) { advanceLexer(); advanceLexer() }
            if (at(T.LPAREN)) skipMatched(T.LPAREN, T.RPAREN) // record/enum sub-patterns
            else if (at(T.IDENTIFIER)) advanceLexer()         // type-pattern binding
        }
        else -> if (!at(T.ARROW) && !at(T.FAT_ARROW) && !at(T.WHEN_KW)) advanceLexer()
    }
    m.done(E.PATTERN)
}

private fun PsiBuilder.atContextualKw(text: String): Boolean =
    at(T.IDENTIFIER) && tokenText == text
