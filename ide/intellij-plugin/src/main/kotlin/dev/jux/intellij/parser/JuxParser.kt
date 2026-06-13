package dev.jux.intellij.parser

import com.intellij.lang.ASTNode
import com.intellij.lang.PsiBuilder
import com.intellij.lang.PsiParser
import com.intellij.psi.tree.IElementType
import com.intellij.psi.tree.TokenSet
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxObservableProps

/**
 * Recursive-descent parser for Jux, built against the platform [PsiBuilder]
 * marker API exactly as IntelliJ's Java parser is.
 *
 * **Scope (current):** the *declaration level* — compilation unit, imports,
 * type declarations and their headers (generics, extends/implements/permits,
 * record components), and members (fields, methods, constructors, enum
 * constants) with full signatures. Method bodies, field initializers, and
 * other expression/statement text are captured as opaque, brace-balanced runs
 * (e.g. [E.CODE_BLOCK]); the full statement/expression grammar replaces those
 * in the next pass. This already yields a tree rich enough for Structure View,
 * folding, and declaration navigation.
 *
 * Recovery never throws: unexpected tokens become error nodes or are skipped,
 * and the parse always reaches EOF.
 */
class JuxParser : PsiParser {
    override fun parse(root: IElementType, builder: PsiBuilder): ASTNode {
        val rootMarker = builder.mark()
        parseFile(builder)
        rootMarker.done(root)
        return builder.treeBuilt
    }

    // ---- compilation unit -------------------------------------------------

    private fun parseFile(b: PsiBuilder) {
        if (b.at(T.PACKAGE_KW)) parsePackage(b)
        while (!b.eof()) {
            when {
                b.at(T.IMPORT_KW) -> parseImport(b)
                isDeclarationStart(b) -> parseDeclaration(b)
                else -> {
                    // Script mode (§E): a file may carry top-level statements
                    // (`print(…)`, `var x = …`, loops, …). Anything that isn't
                    // a declaration parses as a statement; the progress guard
                    // turns a truly stuck token into an error node — and we
                    // re-sync to the next declaration/import so one stray token
                    // doesn't red-flag the rest of the file.
                    val before = b.currentOffset
                    b.parseStatement()
                    if (b.currentOffset == before) {
                        val m = b.mark()
                        b.advanceLexer()
                        while (!b.eof() && !b.at(T.IMPORT_KW) && !isDeclarationStart(b)) b.advanceLexer()
                        m.error("Declaration or statement expected")
                    }
                }
            }
        }
    }

    private fun parsePackage(b: PsiBuilder) {
        val m = b.mark()
        b.advanceLexer() // `package`
        parseQualifiedName(b)
        b.semicolon()
        m.done(E.PACKAGE_STATEMENT)
    }

    private fun parseImport(b: PsiBuilder) {
        val m = b.mark()
        b.advanceLexer() // `import`
        parseQualifiedName(b)
        // Wildcard `.*`, grouped `.{ a, b as c }`, or alias `as Name`.
        if (b.at(T.DOT)) {
            b.advanceLexer()
            if (b.at(T.STAR)) b.advanceLexer()
            else if (b.at(T.LBRACE)) consumeBraceBalanced(b)
        }
        if (b.at(T.AS_KW)) {
            b.advanceLexer()
            b.expectOrError(T.IDENTIFIER, "import alias expected")
        }
        b.semicolon()
        m.done(E.IMPORT_STATEMENT)
    }

    // ---- declarations -----------------------------------------------------

    private fun isDeclarationStart(b: PsiBuilder): Boolean =
        b.at(T.AT) || b.atAny(JUX_MODIFIERS) || b.atAny(JUX_TYPE_DECL_KEYWORDS) ||
            b.at(T.TYPE_KW) || b.at(T.VOID_KW) ||
            b.at(T.NEW_KW) || b.at(T.OPERATOR_KW) ||
            // Ambiguous starts (`Foo bar` decl vs `foo(…)` statement, tuple
            // return types `(int, int) f(…)`, leading method generics
            // `<T> T id(…)`) — settled by a speculative type+name probe so
            // script-mode top-level statements aren't swallowed.
            ((b.at(T.IDENTIFIER) || b.at(T.LPAREN) || b.at(T.LT)) && probeTypeThenName(b))

    /** Speculative: does `‹generics?› Type Name` start here? Always rolls back. */
    private fun probeTypeThenName(b: PsiBuilder): Boolean {
        val probe = b.mark()
        if (b.at(T.LT)) b.skipAngleBalanced()
        b.parseType()
        val ok = b.at(T.IDENTIFIER)
        probe.rollbackTo()
        return ok
    }

    /** Top-level or nested declaration: annotations, modifiers, then a body. */
    private fun parseDeclaration(b: PsiBuilder) {
        val decl = b.mark()
        parseAnnotations(b)
        val sawStatic = parseModifierList(b)
        when {
            b.atAny(JUX_TYPE_DECL_KEYWORDS) -> parseTypeDeclaration(b, decl)
            b.at(T.TYPE_KW) -> parseTypeAlias(b, decl)
            b.at(T.NEW_KW) -> parseConstructor(b, decl)
            b.at(T.OPERATOR_KW) -> parseOperator(b, decl)
            b.at(T.INIT_KW) -> { b.advanceLexer(); parseCodeBlock(b); decl.done(E.INIT_BLOCK) }
            // A bare `{ … }` member: `static { … }` is the §S.4.1 static
            // initializer (runs once, before first observable use); without
            // `static` it's an instance init block.
            b.at(T.LBRACE) -> {
                parseCodeBlock(b)
                decl.done(if (sawStatic) E.STATIC_BLOCK else E.INIT_BLOCK)
            }
            b.at(T.DROP_KW) -> { b.advanceLexer(); parseCodeBlock(b); decl.done(E.DROP_BLOCK) }
            else -> parseMethodOrField(b, decl)
        }
    }

    private fun parseAnnotations(b: PsiBuilder) {
        while (b.at(T.AT)) {
            val m = b.mark()
            b.advanceLexer() // `@`
            parseQualifiedName(b)
            if (b.at(T.LPAREN)) consumeParenBalanced(b)
            m.done(E.ANNOTATION)
        }
    }

    /** Consume the modifier run; reports whether `static` was among them (the
     * bare-`{}` member arm needs it to tell a static initializer from an
     * instance init block). */
    private fun parseModifierList(b: PsiBuilder): Boolean {
        if (!b.atAny(JUX_MODIFIERS)) return false
        var sawStatic = false
        val m = b.mark()
        while (b.atAny(JUX_MODIFIERS)) {
            if (b.at(T.STATIC_KW)) sawStatic = true
            b.advanceLexer()
        }
        m.done(E.MODIFIER_LIST)
        return sawStatic
    }

    private fun parseTypeDeclaration(b: PsiBuilder, decl: PsiBuilder.Marker) {
        val kind = b.tokenType
        b.advanceLexer() // the type keyword
        b.expectOrError(T.IDENTIFIER, "type name expected")
        if (b.at(T.LT)) parseTypeParameters(b)
        if (kind === T.RECORD_KW && b.at(T.LPAREN)) parseRecordComponents(b)
        parseSupertypeClauses(b)
        parseWhereClause(b) // `class Pool<T> where T has …`
        if (b.at(T.LBRACE)) parseClassBody(b, isEnum = kind === T.ENUM_KW)
        decl.done(elementForTypeKeyword(kind))
    }

    private fun parseTypeAlias(b: PsiBuilder, decl: PsiBuilder.Marker) {
        b.advanceLexer() // `type`
        b.expectOrError(T.IDENTIFIER, "alias name expected")
        if (b.at(T.LT)) parseTypeParameters(b)
        b.expectOrError(T.EQ, "'=' expected")
        parseType(b)
        b.semicolon()
        decl.done(E.TYPE_ALIAS_DECLARATION)
    }

    private fun parseSupertypeClauses(b: PsiBuilder) {
        if (b.at(T.EXTENDS_KW)) {
            val m = b.mark()
            b.advanceLexer()
            parseTypeList(b)
            m.done(E.EXTENDS_CLAUSE)
        }
        if (b.at(T.IMPLEMENTS_KW)) {
            val m = b.mark()
            b.advanceLexer()
            parseTypeList(b)
            m.done(E.IMPLEMENTS_CLAUSE)
        }
        if (b.at(T.PERMITS_KW)) {
            val m = b.mark()
            b.advanceLexer()
            parseTypeList(b)
            m.done(E.PERMITS_CLAUSE)
        }
    }

    // ---- class body & members --------------------------------------------

    private fun parseClassBody(b: PsiBuilder, isEnum: Boolean) {
        val body = b.mark()
        b.advanceLexer() // `{`
        if (isEnum) parseEnumConstants(b)
        while (!b.eof() && !b.at(T.RBRACE)) {
            if (isMemberStart(b)) {
                parseDeclaration(b)
            } else {
                // Re-sync: swallow the run of stray tokens up to the next member
                // or `}` as ONE error, so a single bad token doesn't cascade
                // into an error on every token until the brace.
                val m = b.mark()
                b.advanceLexer()
                while (!b.eof() && !b.at(T.RBRACE) && !isMemberStart(b)) b.advanceLexer()
                m.error("Member declaration expected")
            }
        }
        b.expectOrError(T.RBRACE, "'}' expected")
        body.done(E.CLASS_BODY)
    }

    private fun isMemberStart(b: PsiBuilder): Boolean =
        b.at(T.AT) || b.atAny(JUX_MODIFIERS) || b.atAny(JUX_TYPE_DECL_KEYWORDS) ||
            b.at(T.TYPE_KW) || // nested `type Alias = T;` (parseDeclaration handles it)
            b.at(T.NEW_KW) || b.at(T.OPERATOR_KW) || b.at(T.DROP_KW) || b.at(T.INIT_KW) ||
            b.at(T.LBRACE) || b.at(T.IDENTIFIER) || b.at(T.VOID_KW) ||
            b.at(T.LPAREN) || b.at(T.LT) // tuple-typed member / bare generic method

    private fun parseEnumConstants(b: PsiBuilder) {
        while (!b.eof() && b.at(T.IDENTIFIER)) {
            val c = b.mark()
            b.advanceLexer() // name
            if (b.at(T.LPAREN)) consumeParenBalanced(b)        // payload variant
            else if (b.at(T.EQ)) { b.advanceLexer(); b.consumeBalancedUntil(ENUM_SEP) } // explicit discriminator
            c.done(E.ENUM_CONSTANT)
            if (b.at(T.COMMA)) b.advanceLexer() else break
        }
        if (b.at(T.SEMICOLON)) b.advanceLexer() // separator before methods
    }

    private fun parseConstructor(b: PsiBuilder, decl: PsiBuilder.Marker) {
        b.advanceLexer() // `new`
        if (b.at(T.LPAREN)) parseParameterList(b)
        parseThrows(b)
        parseWhereClause(b)
        parseBodyOrSemicolon(b)
        decl.done(E.CONSTRUCTOR_DECLARATION)
    }

    private fun parseOperator(b: PsiBuilder, decl: PsiBuilder.Marker) {
        b.advanceLexer() // `operator`
        parseOperatorSymbolAndRest(b)
        decl.done(E.OPERATOR_DECLARATION)
    }

    /**
     * After the `operator` keyword: the operator's symbol, then the parameter
     * list, throws/where clauses, and body. The call operator `()` needs care —
     * its symbol IS a paren pair (`operator ()(int x)`), so a `()` immediately
     * followed by another `(` is consumed as the symbol rather than mistaken
     * for an empty parameter list.
     */
    private fun parseOperatorSymbolAndRest(b: PsiBuilder) {
        if (b.at(T.LPAREN) && b.lookAhead(1) === T.RPAREN && b.lookAhead(2) === T.LPAREN) {
            b.advanceLexer() // `(`
            b.advanceLexer() // `)`
        } else {
            // Any other symbol (`+`, `[]`, `string`, `hash`, `<=>`, …): consume up to `(`.
            while (!b.eof() && !b.at(T.LPAREN) && !b.at(T.LBRACE) && !b.at(T.SEMICOLON)) b.advanceLexer()
        }
        if (b.at(T.LPAREN)) parseParameterList(b)
        parseThrows(b)
        parseWhereClause(b)
        parseBodyOrSemicolon(b)
    }

    /**
     * A member that begins with a type: either a method (`T name(...)`), a
     * named constructor (`TypeName(...)` with no return type), or a field
     * (`T name [= …];`).
     */
    private fun parseMethodOrField(b: PsiBuilder, decl: PsiBuilder.Marker) {
        // Java-style leading method generics: `public <T extends Shape> T pick(…)`.
        if (b.at(T.LT)) parseTypeParameters(b)
        parseType(b)
        // `ReturnType operator <op>( … )` — operator overload after the type.
        if (b.at(T.OPERATOR_KW)) {
            b.advanceLexer() // `operator`
            parseOperatorSymbolAndRest(b)
            decl.done(E.OPERATOR_DECLARATION)
            return
        }
        if (b.at(T.LPAREN)) {
            // The "type" we read was actually the constructor name.
            parseParameterList(b)
            parseThrows(b)
            parseWhereClause(b)
            parseBodyOrSemicolon(b)
            decl.done(E.CONSTRUCTOR_DECLARATION)
            return
        }
        b.expectOrError(T.IDENTIFIER, "name expected")
        if (b.at(T.LT)) parseTypeParameters(b) // method type params after name
        if (b.at(T.LPAREN)) {
            parseParameterList(b)
            parseThrows(b)
            parseWhereClause(b)
            parseBodyOrSemicolon(b)
            decl.done(E.METHOD_DECLARATION)
        } else {
            // Field or property (§M.7 base syntax + §P observability).
            when {
                // `Type Name { get; set; } [= init] ;?` — accessor block. The
                // probe peeks past `{` and rolls back; on failure the brace is
                // treated exactly as before (opaque block under a field), so
                // non-property braces can't regress.
                b.at(T.LBRACE) && looksLikeAccessorBlock(b) -> {
                    parsePropertyAccessorList(b)
                    if (b.at(T.EQ)) { b.advanceLexer(); b.parseExpression() }
                    // The trailing `;` after `}` (or after the initializer) is
                    // optional — mirrors juxc-parse, no error when absent.
                    if (b.at(T.SEMICOLON)) b.advanceLexer()
                    decl.done(E.PROPERTY_DECLARATION)
                }
                // `Type Name -> expr;` — read-only computed shorthand,
                // equivalent to `{ get -> expr; }` (§M.7.4). `=>` is the
                // type-test operator, tolerated here only for error recovery.
                b.at(T.ARROW) || b.at(T.FAT_ARROW) -> {
                    b.advanceLexer()
                    b.parseExpression()
                    b.semicolon()
                    decl.done(E.PROPERTY_DECLARATION)
                }
                else -> {
                    // Plain field: optional `= expr` initializer.
                    if (b.at(T.LBRACE)) {
                        parseCodeBlock(b) // stray brace — legacy fallback
                    } else {
                        if (b.at(T.EQ)) { b.advanceLexer(); b.parseExpression() }
                        b.semicolon()
                    }
                    decl.done(E.FIELD_DECLARATION)
                }
            }
        }
    }

    // ---- observable properties (§P) ----------------------------------------

    /**
     * Speculative probe: does the `{` at the cursor open a property accessor
     * block? True when the first significant tokens inside are an optional
     * visibility run followed by `get`/`set` (contextual identifiers), the
     * removed-but-diagnosed `init` keyword, or an immediate `}` (an empty block
     * the user is still typing). Always rolls back.
     */
    private fun looksLikeAccessorBlock(b: PsiBuilder): Boolean {
        val probe = b.mark()
        b.advanceLexer() // `{`
        while (b.atAny(ACCESSOR_VISIBILITY)) b.advanceLexer()
        val ok = b.at(T.RBRACE) || b.at(T.INIT_KW) ||
            (b.at(T.IDENTIFIER) && b.tokenText in JuxObservableProps.ACCESSOR_KINDS)
        probe.rollbackTo()
        return ok
    }

    /** The `{ accessor+ }` braces of a property declaration. */
    private fun parsePropertyAccessorList(b: PsiBuilder) {
        val list = b.mark()
        b.advanceLexer() // `{`
        while (!b.eof() && !b.at(T.RBRACE)) {
            if (atAccessorStart(b)) {
                parsePropertyAccessor(b)
            } else {
                // Recovery: skip one token so the loop always progresses.
                val e = b.mark()
                b.advanceLexer()
                e.error("accessor expected ('get' or 'set')")
            }
        }
        b.expectOrError(T.RBRACE, "'}' expected")
        list.done(E.PROPERTY_ACCESSOR_LIST)
    }

    private fun atAccessorStart(b: PsiBuilder): Boolean =
        b.atAny(ACCESSOR_VISIBILITY) || b.at(T.INIT_KW) ||
            (b.at(T.IDENTIFIER) && b.tokenText in JuxObservableProps.ACCESSOR_KINDS)

    /**
     * One accessor: `[public|protected|private] (get|set) body` where the body
     * is `;` (auto), `-> expr ;` (expression form), or `{ … }` (block form).
     * Mirrors `juxc-parse` decls.rs. The removed `init` accessor parses through
     * with an error marker so the tree stays well-shaped while the user fixes it.
     */
    private fun parsePropertyAccessor(b: PsiBuilder) {
        val m = b.mark()
        if (b.atAny(ACCESSOR_VISIBILITY)) {
            val mods = b.mark()
            while (b.atAny(ACCESSOR_VISIBILITY)) b.advanceLexer()
            mods.done(E.MODIFIER_LIST)
        }
        when {
            b.at(T.INIT_KW) -> {
                val err = b.mark()
                b.advanceLexer()
                err.error(
                    "the 'init' accessor was removed (§P) — use '{ get; }' for a " +
                        "read-only property settable in the constructor",
                )
            }
            b.at(T.IDENTIFIER) && b.tokenText in JuxObservableProps.ACCESSOR_KINDS ->
                b.advanceLexer() // `get` / `set`
            else -> b.errorHere("'get' or 'set' expected")
        }
        when {
            b.at(T.SEMICOLON) -> b.advanceLexer() // auto accessor: `get;`
            b.at(T.ARROW) || b.at(T.FAT_ARROW) -> {
                // Expression body: `get -> _age;` (lenient on `=>` for recovery).
                b.advanceLexer()
                b.parseExpression()
                b.semicolon()
            }
            b.at(T.LBRACE) -> b.parseBlock() // full block body (`value` is a plain identifier inside)
            else -> b.errorHere("';', '-> expression;' or '{ … }' expected for accessor body")
        }
        m.done(E.PROPERTY_ACCESSOR)
    }

    private fun parseThrows(b: PsiBuilder) {
        if (!b.at(T.THROWS_KW)) return
        val m = b.mark()
        b.advanceLexer()
        parseTypeList(b)
        m.done(E.THROWS_CLAUSE)
    }

    /**
     * Generic constraint clause (§T.10): `where T has operator<=>(T) -> int`.
     * `where` is contextual (an identifier, not a keyword). The constraint
     * grammar is rich, so the clause body is consumed as a balanced run up to
     * the declaration's body/terminator — structure lands with a later pass.
     */
    private fun parseWhereClause(b: PsiBuilder) {
        if (!b.at(T.IDENTIFIER) || b.tokenText != "where") return
        val m = b.mark()
        b.advanceLexer() // `where`
        b.consumeBalancedUntil(WHERE_CLAUSE_END)
        m.done(E.WHERE_CLAUSE)
    }

    /** A `{ … }` body, a `= expr;` single-expression body, or a bare `;`. */
    private fun parseBodyOrSemicolon(b: PsiBuilder) {
        when {
            b.at(T.LBRACE) -> parseCodeBlock(b)
            b.at(T.EQ) -> { b.advanceLexer(); b.parseExpression(); b.semicolon() }
            else -> b.semicolon()
        }
    }

    private fun parseParameterList(b: PsiBuilder) {
        val m = b.mark()
        b.expectOrError(T.LPAREN, "'(' expected")
        if (!b.at(T.RPAREN)) {
            parseParameter(b)
            while (b.at(T.COMMA)) { b.advanceLexer(); parseParameter(b) }
        }
        b.expectOrError(T.RPAREN, "')' expected")
        m.done(E.PARAMETER_LIST)
    }

    /** `annotation* (final|const|ref|weak|out)* type '...'? name (= expr)?` */
    private fun parseParameter(b: PsiBuilder) {
        val p = b.mark()
        parseAnnotations(b)
        // Leading param modifiers, any order: `final` / `const` / `ref` (§M.13)
        // / `weak` (§M.14.3, e.g. `weak Counter c`).
        while (b.at(T.FINAL_KW) || b.at(T.CONST_KW) || b.atRefKw() || b.at(T.WEAK_KW)) b.advanceLexer()
        if (b.at(T.IDENTIFIER) && b.tokenText == "out") b.advanceLexer() // contextual param-mode
        b.parseType()
        if (b.at(T.ELLIPSIS)) b.advanceLexer() // `T... name` varargs
        b.expectOrError(T.IDENTIFIER, "parameter name expected")
        if (b.at(T.EQ)) { b.advanceLexer(); b.parseExpression() }
        p.done(E.PARAMETER)
    }

    /** A method / initializer body — a real statement block (see JuxStatements). */
    private fun parseCodeBlock(b: PsiBuilder) = b.parseBlock()

    private fun consumeParenBalanced(b: PsiBuilder) = b.skipMatched(T.LPAREN, T.RPAREN)
    private fun consumeBraceBalanced(b: PsiBuilder) = b.skipMatched(T.LBRACE, T.RBRACE)

    // ---- types & names (delegate to shared top-level parsers) -------------

    private fun parseQualifiedName(b: PsiBuilder) = b.parseQualifiedName()
    private fun parseType(b: PsiBuilder) = b.parseType()
    private fun parseTypeList(b: PsiBuilder) = b.parseTypeList()
    private fun parseTypeParameters(b: PsiBuilder) = b.parseTypeParameters()

    private fun parseRecordComponents(b: PsiBuilder) {
        val m = b.mark()
        b.skipMatched(T.LPAREN, T.RPAREN)
        m.done(E.RECORD_COMPONENT_LIST)
    }

    private fun elementForTypeKeyword(kw: IElementType?): IElementType = when (kw) {
        T.CLASS_KW -> E.CLASS_DECLARATION
        T.INTERFACE_KW -> E.INTERFACE_DECLARATION
        T.ENUM_KW -> E.ENUM_DECLARATION
        T.RECORD_KW -> E.RECORD_DECLARATION
        T.STRUCT_KW -> E.STRUCT_DECLARATION
        T.ANNOTATION_KW -> E.ANNOTATION_DECLARATION
        else -> E.CLASS_DECLARATION
    }

    private companion object {
        val ENUM_SEP: TokenSet = TokenSet.create(T.COMMA, T.SEMICOLON)

        /** Per-accessor visibility (§P.1.3) — no package level on accessors. */
        val ACCESSOR_VISIBILITY: TokenSet = TokenSet.create(T.PUBLIC_KW, T.PROTECTED_KW, T.PRIVATE_KW)

        /** Tokens ending a `where` constraint run: the body, terminator, or `= expr`. */
        val WHERE_CLAUSE_END: TokenSet = TokenSet.create(T.LBRACE, T.SEMICOLON, T.EQ)
    }
}
