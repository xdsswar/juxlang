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
        // A member modifier cannot begin a statement: the block's `}` is
        // missing, and what follows belongs to the enclosing type. Stop here
        // so the next members still parse as members.
        if (atAny(MEMBER_ONLY_START)) break
        val before = currentOffset
        parseStatement()
        if (currentOffset == before) { // no progress — skip a token to recover
            val e = mark()
            val message = unexpectedTokenMessage()
            val orphanClause = at(T.CATCH_KW) || at(T.FINALLY_KW)
            advanceLexer()
            // An orphan `catch (E e)` takes its header with it; its block then
            // parses as an ordinary block, so the mistake is one error.
            if (orphanClause && at(T.LPAREN)) skipMatched(T.LPAREN, T.RPAREN)
            e.error(message)
        }
    }
    closeBrace()
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
        // `yield expr;` — generators (§M.2) are specced but not yet compiled;
        // the lenient-superset parser accepts the statement so the reserved
        // keyword never paints a red squiggle (the compiler diagnoses it).
        T.YIELD_KW -> parseSimple(E.EXPRESSION_STATEMENT, hasOptionalExpr = true)
        T.BREAK_KW -> parseBreakContinue(E.BREAK_STATEMENT)
        T.CONTINUE_KW -> parseBreakContinue(E.CONTINUE_STATEMENT)
        T.TRY_KW -> parseTryStatement()
        T.UNSAFE_KW -> { val m = mark(); advanceLexer(); parseBlock(); m.done(E.UNSAFE_STATEMENT) }
        T.SEMICOLON -> { val m = mark(); advanceLexer(); m.done(E.EMPTY_STATEMENT) }
        T.VAR_KW, T.FINAL_KW, T.CONST_KW -> parseLocalVariable()
        // `ref Type name = …;` (reference local) — token exists only once the
        // compiler reserves `ref`; see JUX_REF_KW.
        else -> if (atRefKw()) parseLocalVariable() else parseLabeledOrExprOrLocal()
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
    if (atContextualKw("cfg")) {
        // Compile-time `if cfg(pred)`: the predicate nests `key = "value"`
        // leaves (`any(os = "linux")`), which are not expressions.
        advanceLexer()
        if (at(T.LPAREN)) skipMatched(T.LPAREN, T.RPAREN) else error("'(' expected")
    } else if (expect(T.LPAREN)) {
        parseExpression()
        expectOrError(T.RPAREN, "')' expected")
    } else {
        // `if x > 0 {`: the condition is still read, so the body parses.
        errorHere("'(' expected")
        parseExpression()
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
    } else {
        errorHere("'(' expected")
        parseExpression()
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

/**
 * The `for (T item : xs)` binding. The variable gets a real
 * [E.LOCAL_VARIABLE] node: without one it could not be completed, navigated to,
 * renamed, or counted as used — the loop variable you just wrote never appeared
 * in the popup.
 */
private fun PsiBuilder.tryForEachHeader(): Boolean {
    val v = mark()
    while (at(T.FINAL_KW) || at(T.CONST_KW) || atRefKw()) advanceLexer()
    if (at(T.VAR_KW)) advanceLexer() else parseType()
    if (!at(T.IDENTIFIER)) {
        // Not a for-each after all; the caller rolls the whole header back.
        v.drop()
        return false
    }
    advanceLexer() // name
    v.done(E.LOCAL_VARIABLE)
    return true
}

private fun PsiBuilder.parseForInit() {
    if (at(T.VAR_KW) || at(T.FINAL_KW) || at(T.CONST_KW) || atRefKw()) {
        val v = mark()
        while (at(T.FINAL_KW) || at(T.CONST_KW) || atRefKw()) advanceLexer()
        if (at(T.VAR_KW)) advanceLexer() else parseType()
        if (at(T.IDENTIFIER)) advanceLexer()
        if (expect(T.EQ)) parseExpression()
        v.done(E.LOCAL_VARIABLE)
        return
    }
    // Typed init `int i = 0` (speculative) or expression list.
    val p = mark()
    val v = mark()
    parseType()
    if (at(T.IDENTIFIER)) {
        advanceLexer()
        if (expect(T.EQ)) parseExpression()
        v.done(E.LOCAL_VARIABLE)
        p.drop()
        return
    }
    v.drop()
    p.rollbackTo()
    parseExpression()
    while (at(T.COMMA)) { advanceLexer(); parseExpression() }
}

private fun PsiBuilder.parseLocalVariable() {
    val m = mark()
    while (at(T.FINAL_KW) || at(T.CONST_KW) || atRefKw()) advanceLexer()
    val isVar = at(T.VAR_KW)
    if (isVar) advanceLexer() else parseType()
    // Destructuring (§5.4): `var (x, y) = t;` or `var Pt(x, y) = p;`.
    if (isVar && (at(T.LPAREN) || atRecordPatternHead())) {
        parseBindingPattern()
        if (expect(T.EQ)) parseExpressionOrError()
        semicolon()
        m.done(E.DESTRUCTURING_DECLARATION)
        return
    }
    if (at(T.LPAREN)) skipMatched(T.LPAREN, T.RPAREN) // typed tuple destructuring
    else expectOrError(T.IDENTIFIER, "Variable name expected")
    if (expect(T.EQ)) parseExpressionOrError()
    semicolon()
    m.done(E.LOCAL_VARIABLE)
}

/** `Pt(` or `geo.Pt(`: a record pattern's type name, then its components. */
private fun PsiBuilder.atRecordPatternHead(): Boolean {
    if (!at(T.IDENTIFIER)) return false
    var i = 1
    while (lookAhead(i) === T.DOT && lookAhead(i + 1) === T.IDENTIFIER) i += 2
    return lookAhead(i) === T.LPAREN
}

/**
 * One binding pattern of a destructuring declaration: a tuple `(p, q)`, a
 * record `Pt(p, q)`, or a binder `x` / `var x` / `int x`. Each binder is a
 * LOCAL_VARIABLE node.
 */
private fun PsiBuilder.parseBindingPattern() {
    when {
        at(T.LPAREN) -> parseBindingList()
        atRecordPatternHead() -> {
            val type = mark()
            advanceLexer()
            while (at(T.DOT) && lookAhead(1) === T.IDENTIFIER) { advanceLexer(); advanceLexer() }
            type.done(E.TYPE_REFERENCE)
            parseBindingList()
        }
        else -> {
            val binder = mark()
            while (at(T.FINAL_KW) || at(T.VAR_KW)) advanceLexer()
            // A typed binder `int x`: a type, then the name.
            val typed = mark()
            parseType()
            if (at(T.IDENTIFIER)) typed.drop() else typed.rollbackTo()
            expectOrError(T.IDENTIFIER, "Binding name expected")
            binder.done(E.LOCAL_VARIABLE)
        }
    }
}

private fun PsiBuilder.parseBindingList() {
    advanceLexer() // `(`
    if (!at(T.RPAREN)) {
        parseBindingPattern()
        while (at(T.COMMA)) { advanceLexer(); if (!at(T.RPAREN)) parseBindingPattern() }
    }
    expectOrError(T.RPAREN, "')' expected")
}

private fun PsiBuilder.parseLabeledOrExprOrLocal() {
    if (atAssertStatement()) {
        val m = mark()
        advanceLexer() // `assert`
        parseExpression()
        if (at(T.COLON)) { advanceLexer(); parseExpression() } // `: message`
        semicolon()
        m.done(E.EXPRESSION_STATEMENT)
        return
    }
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
        if (parseExpression() == null) {
            // Nothing here starts an expression: leave the token to the
            // block, which names it (`'else' without 'if'`).
            e.drop()
            return
        }
        semicolon()
        e.done(E.EXPRESSION_STATEMENT)
    }
}

/** Parse `Type name [= expr];`; returns false (for rollback) if it isn't one. */
private fun PsiBuilder.tryLocalVarTail(): Boolean {
    parseType()
    if (!at(T.IDENTIFIER)) return false
    advanceLexer() // name
    if (expect(T.EQ)) parseExpressionOrError()
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
            // Multi-catch: `catch (NetError | TimeoutError e)`. The caught
            // value is a binding like any other and gets its own node.
            val v = mark()
            parseType()
            while (at(T.PIPE)) { advanceLexer(); parseType() }
            expectOrError(T.IDENTIFIER, "Exception name expected")
            v.done(E.LOCAL_VARIABLE)
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
        if (currentOffset == before) {
            val e = mark()
            val message = unexpectedTokenMessage()
            advanceLexer()
            e.error(message)
        }
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
            if (at(T.WHEN_KW)) { val g = mark(); advanceLexer(); parseGuardExpression(); g.done(E.PATTERN_GUARD) }
        }
        at(T.DEFAULT_KW) -> advanceLexer()
        else -> { m.drop(); return }
    }
    // `->` (lenient: also accept `=>`) then an expression `;` or a block. A
    // Java-style `case 1:` marks the `:` and parses on as if `->` were there.
    if (!expect(T.ARROW) && !expect(T.FAT_ARROW)) {
        if (at(T.COLON)) {
            val colon = mark()
            advanceLexer()
            colon.error("'->' expected")
        } else {
            errorHere("'->' expected")
        }
    }
    // `default -> throw new IllegalStateException(..);` is a statement arm.
    when {
        at(T.LBRACE) -> parseBlock()
        at(T.THROW_KW) -> parseStatement()
        else -> { parseExpressionOrError(); semicolon() }
    }
    m.done(E.SWITCH_CASE)
}

/**
 * A lenient pattern (Grammar §A.3): `var name`, a literal (with optional
 * range), a tuple `(p, q)`, a qualified name optionally followed by a
 * sub-pattern list `Circle(var r, _)`, or a type test `Dog d`.
 *
 * Every name a pattern binds (`var r`, the `d` of `Dog d`) is a
 * LOCAL_VARIABLE node, so an arm's binders resolve, rename, complete and
 * find their usages like any local. Sub-patterns are PATTERN nodes of their
 * own, which is what lets a `var` binder find the record component it binds.
 * A pattern's head name (`Circle`, `Color.Red`, a bare `Sat`) stays plain
 * tokens: it may name a type or an enum variant of the subject, which only
 * the subject's type decides.
 */
private fun PsiBuilder.parsePattern() {
    val m = mark()
    while (at(T.FINAL_KW) || at(T.CONST_KW)) advanceLexer()
    // `var name`: a binder.
    if (at(T.VAR_KW) && lookAhead(1) === T.IDENTIFIER) {
        val binder = mark()
        advanceLexer()
        advanceLexer()
        binder.done(E.LOCAL_VARIABLE)
        m.done(E.PATTERN)
        return
    }
    if (at(T.VAR_KW)) advanceLexer()
    // `Dog d`: a type test with a binder. The type is a real type reference.
    if (at(T.IDENTIFIER) && lookAhead(1) === T.IDENTIFIER) {
        val binder = mark()
        val type = mark()
        advanceLexer()
        type.done(E.TYPE_REFERENCE)
        advanceLexer()
        binder.done(E.LOCAL_VARIABLE)
        m.done(E.PATTERN)
        return
    }
    // A literal bound, allowing a leading minus (`-5`).
    fun literalBound(): Boolean {
        if (at(T.MINUS) && T.LITERALS.contains(lookAhead(1))) { advanceLexer(); advanceLexer(); return true }
        if (T.LITERALS.contains(tokenType)) { advanceLexer(); return true }
        return false
    }
    when {
        // `..0` / `..=0`: a range open at the bottom.
        at(T.DOT_DOT) || at(T.DOT_DOT_EQ) -> { advanceLexer(); literalBound() }
        T.LITERALS.contains(tokenType) || (at(T.MINUS) && T.LITERALS.contains(lookAhead(1))) -> {
            literalBound()
            // `0..10`, `0..=10`, or `10..` open at the top.
            if (at(T.DOT_DOT) || at(T.DOT_DOT_EQ)) {
                advanceLexer()
                literalBound()
            }
        }
        at(T.IDENTIFIER) -> {
            // qualified name
            advanceLexer()
            while (at(T.DOT) && lookAhead(1) === T.IDENTIFIER) { advanceLexer(); advanceLexer() }
            if (at(T.LPAREN)) parseSubPatterns()      // record/enum sub-patterns
            else if (at(T.IDENTIFIER)) advanceLexer() // `a.B x`: not in the grammar, kept lenient
        }
        // tuple pattern `(p, q)`
        at(T.LPAREN) -> parseSubPatterns()
        else -> if (!at(T.ARROW) && !at(T.FAT_ARROW) && !at(T.WHEN_KW)) advanceLexer()
    }
    m.done(E.PATTERN)
}

/**
 * `(p, q, …)`: the sub-patterns of a record, enum variant or tuple pattern,
 * each a PATTERN of its own. Stops at `)`, or at a token that cannot continue
 * a pattern list so a half-typed arm never swallows the rest of the switch.
 */
private fun PsiBuilder.parseSubPatterns() {
    advanceLexer() // `(`
    while (!eof() && !at(T.RPAREN)) {
        val before = currentOffset
        parsePattern()
        if (at(T.COMMA)) advanceLexer()
        else if (currentOffset == before || at(T.ARROW) || at(T.LBRACE) || at(T.SEMICOLON)) break
    }
    expectOrError(T.RPAREN, "')' expected")
}

/**
 * `assert` as a statement (S.7.2): followed by a condition that is not a
 * parenthesized call argument list, or by `(…)` and then more of an
 * expression or `: message`. A plain `assert(x);` stays the built-in call.
 */
private fun PsiBuilder.atAssertStatement(): Boolean {
    if (!atContextualKw("assert")) return false
    if (lookAhead(1) !== T.LPAREN) return lookAhead(1) !== T.SEMICOLON && lookAhead(1) !== T.DOT
    // `assert(x);` / `assert(x, "why");` is the built-in call: `;` follows
    // its `)`. Anything else continues an expression that merely starts with
    // parentheses, `assert (a % 2) == 1 : "odd";`, so it is the statement.
    val probe = mark()
    advanceLexer()
    skipMatched(T.LPAREN, T.RPAREN)
    val statement = !at(T.SEMICOLON) && !eof()
    probe.rollbackTo()
    return statement
}

private fun PsiBuilder.atContextualKw(text: String): Boolean =
    at(T.IDENTIFIER) && tokenText == text
