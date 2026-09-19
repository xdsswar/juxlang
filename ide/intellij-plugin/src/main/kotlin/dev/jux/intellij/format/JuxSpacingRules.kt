package dev.jux.intellij.format

import com.intellij.formatting.Block
import com.intellij.formatting.Spacing
import com.intellij.formatting.SpacingBuilder
import com.intellij.lang.ASTNode
import com.intellij.psi.TokenType
import com.intellij.psi.codeStyle.CodeStyleSettings
import com.intellij.psi.tree.IElementType
import com.intellij.psi.codeStyle.CommonCodeStyleSettings
import com.intellij.psi.tree.TokenSet
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Spacing policy: a [SpacingBuilder] for everything expressible as token/parent
 * rules, plus [custom] for the few shapes the builder can't see (cuddled
 * `} else` keywords, blank-line clamping between siblings, the forced newline
 * after package/imports).
 *
 * **Rule order is load-bearing** — the first matching rule wins, and `< > * &
 * ?` are all overloaded (generics vs comparison, type suffix vs ternary, …):
 *   ① generics  ② member access  ③ separators  ④ type suffixes / postfix
 *   ⑤ unary  ⑥ parens/calls  ⑦ keywords  ⑧ braces (K&R)  ⑨ binary operators
 *   ⑩ arrows  ⑪ ternary/colons  ⑫ annotations.
 *
 * Line-break policy: **preserve the user's line breaks** except where a
 * Code Style option says otherwise, under Java's names: braces placement,
 * `else`/`catch`/`finally`/`while` on a new line, the "Keep simple ... in
 * one line" options, and the "Minimum blank lines" of the Blank Lines tab.
 * Jux's defaults keep the layout as written (no minimum blank lines, simple
 * bodies kept on one line), so the examples corpus's deliberate one-liner
 * interfaces survive reformat.
 */
object JuxSpacingRules {

    fun create(settings: CodeStyleSettings, common: CommonCodeStyleSettings): SpacingBuilder {
        return SpacingBuilder(settings, JuxLanguage)
            // ① generics — zero space at the angle delimiters and before the list.
            .afterInside(T.LT, E.TYPE_ARGUMENT_LIST).spaces(0)
            .afterInside(T.LT, E.TYPE_PARAMETER_LIST).spaces(0)
            .beforeInside(GT_CLOSERS, E.TYPE_ARGUMENT_LIST).spaces(0)
            .beforeInside(GT_CLOSERS, E.TYPE_PARAMETER_LIST).spaces(0)
            .before(E.TYPE_ARGUMENT_LIST).spaces(0)
            .before(E.TYPE_PARAMETER_LIST).spaces(0)
            // ② member access — never spaced.
            .around(T.DOT).spaces(0)
            .around(T.QUESTION_DOT).spaces(0)
            .around(T.COLON_COLON).spaces(0)
            // ③ separators — tight-left, spaced-right.
            .before(T.COMMA).spaces(0)
            .after(T.COMMA).spaceIf(common.SPACE_AFTER_COMMA)
            .before(T.SEMICOLON).spaces(0)
            // ④ type suffixes (`int?`, `Foo*`, `T...`) and postfix `?` / `!!`.
            .beforeInside(T.QUESTION, E.TYPE_REFERENCE).spaces(0)
            .beforeInside(T.STAR, E.TYPE_REFERENCE).spaces(0)
            .beforeInside(T.QUESTION, E.POSTFIX_EXPRESSION).spaces(0)
            .beforeInside(T.BANG_BANG, E.POSTFIX_EXPRESSION).spaces(0)
            .beforeInside(INC_DEC_OPS, E.POSTFIX_EXPRESSION).spaces(0)
            .before(T.ELLIPSIS).spaces(0)
            .after(T.ELLIPSIS).spaces(1)
            // ⑤ unary operators bind to their operand.
            .afterInside(UNARY_OPS, E.UNARY_EXPRESSION).spaces(0)
            // ⑥ parens & call shapes — tight interiors, no gap before arg lists.
            .before(E.ARGUMENT_LIST).spaces(0)
            .before(E.PARAMETER_LIST).spaces(0)
            .after(T.LPAREN).spaces(0)
            .before(T.RPAREN).spaces(0)
            .after(T.LBRACKET).spaces(0)
            .before(T.RBRACKET).spaces(0)
            .before(T.LBRACKET).spaces(0)
            // ⑦ statement keywords read with a following space.
            .after(KEYWORDS_THEN_SPACE).spaces(1)
            .around(CLAUSE_KEYWORDS).spaces(1)
            // ⑧ braces — K&R: `) {`, `name {`; same-line interiors get one space.
            .before(E.CODE_BLOCK).spaces(1)
            .before(E.CLASS_BODY).spaces(1)
            .beforeInside(T.LBRACE, E.SWITCH_STATEMENT).spaces(1)
            .beforeInside(T.LBRACE, E.SWITCH_EXPRESSION).spaces(1)
            .afterInside(T.LBRACE, E.CLASS_BODY).spaces(1)
            .beforeInside(T.RBRACE, E.CLASS_BODY).spaces(1)
            .afterInside(T.LBRACE, E.CODE_BLOCK).spaces(1)
            .beforeInside(T.RBRACE, E.CODE_BLOCK).spaces(1)
            // §L.7 native block (`unsafe native { … }`) and §P accessor list
            // (`name { get; set; }`) — same K&R one-space brace policy.
            .beforeInside(T.LBRACE, E.EXTERN_BLOCK).spaces(1)
            .afterInside(T.LBRACE, E.EXTERN_BLOCK).spaces(1)
            .beforeInside(T.RBRACE, E.EXTERN_BLOCK).spaces(1)
            .before(E.PROPERTY_ACCESSOR_LIST).spaces(1)
            // ⑨ binary / assignment operators — spaced, scoped to expression
            //    nodes so the overloaded tokens can't leak into other shapes.
            .around(ASSIGN_OPS).spaceIf(common.SPACE_AROUND_ASSIGNMENT_OPERATORS)
            .aroundInside(LOGICAL_OPS, E.BINARY_EXPRESSION).spaceIf(common.SPACE_AROUND_LOGICAL_OPERATORS)
            .aroundInside(EQUALITY_OPS, E.BINARY_EXPRESSION).spaceIf(common.SPACE_AROUND_EQUALITY_OPERATORS)
            .aroundInside(RELATIONAL_OPS, E.BINARY_EXPRESSION).spaceIf(common.SPACE_AROUND_RELATIONAL_OPERATORS)
            .aroundInside(SHIFT_OPS, E.BINARY_EXPRESSION).spaceIf(common.SPACE_AROUND_SHIFT_OPERATORS)
            .aroundInside(ADDITIVE_OPS, E.BINARY_EXPRESSION).spaceIf(common.SPACE_AROUND_ADDITIVE_OPERATORS)
            .aroundInside(MULTIPLICATIVE_OPS, E.BINARY_EXPRESSION).spaceIf(common.SPACE_AROUND_MULTIPLICATIVE_OPERATORS)
            .aroundInside(BITWISE_OPS, E.BINARY_EXPRESSION).spaceIf(common.SPACE_AROUND_BITWISE_OPERATORS)
            .aroundInside(ELVIS_OPS, E.BINARY_EXPRESSION).spaces(1)
            .aroundInside(T.SPACESHIP, E.BINARY_EXPRESSION).spaces(1)
            // Multi-catch `A | B` and or-patterns `case A | B`.
            .aroundInside(T.PIPE, E.CATCH_CLAUSE).spaces(1)
            .aroundInside(T.PIPE, E.SWITCH_CASE).spaces(1)
            // Ranges stay tight: `0..10`.
            .aroundInside(TokenSet.create(T.DOT_DOT, T.DOT_DOT_EQ), E.RANGE_EXPRESSION).spaces(0)
            // ⑩ arrows: lambdas / case arms (`->`) and type-tests (`=>`).
            .around(T.ARROW).spaces(1)
            .around(T.FAT_ARROW).spaces(1)
            // ⑪ colons: named args tight-left, for-each / ternary spaced,
            //    labels tight-left.
            .beforeInside(T.COLON, E.ARGUMENT_LIST).spaces(0)
            .afterInside(T.COLON, E.ARGUMENT_LIST).spaces(1)
            .beforeInside(T.COLON, E.LABELED_STATEMENT).spaces(0)
            .aroundInside(T.COLON, E.FOR_EACH_STATEMENT).spaces(1)
            .aroundInside(T.QUESTION, E.CONDITIONAL_EXPRESSION).spaces(1)
            .aroundInside(T.COLON, E.CONDITIONAL_EXPRESSION).spaces(1)
            // ⑫ annotations: `@` binds to its name.
            .after(T.AT).spaces(0)
    }

    /**
     * The shapes the SpacingBuilder can't express. Checked first; `null`
     * falls through to the builder.
     */
    fun custom(parent: JuxBlock, left: Block?, right: Block, ctx: JuxFormatContext): Spacing? {
        val l = (left as? JuxBlock)?.node ?: return null
        val r = (right as? JuxBlock)?.node ?: return null
        val p = parent.node.elementType
        val common = ctx.common

        // Cuddled keywords: `} else {`, `} catch`, `} finally`, `} while`.
        // Java's "'else' on new line" and its kin: on, the keyword starts its
        // own line; off, it joins the `}` before it. After a statement that is
        // not a block (`if (x) a();\nelse b();`) a line break is kept.
        cuddledKeywordOption(p, r, common)?.let { onNewLine ->
            return when {
                onNewLine -> Spacing.createSpacing(0, 0, 1, false, 0)
                l.elementType === E.CODE_BLOCK || l.elementType === E.CATCH_CLAUSE -> Spacing.createSpacing(1, 1, 0, false, 0)
                else -> Spacing.createSpacing(1, 1, 0, true, 0)
            }
        }

        // Braces placement: where the `{` of a body goes.
        braceSpacing(parent.node, l, r, ctx)?.let { return it }

        // Headers: at least one line break after `package` and each `import`,
        // then the "After package", "Before imports" and "After imports"
        // minimum blank lines. A trailing same-line comment
        // (`import a.b.C; // why`) is exempt: forcing the break would tear
        // the comment onto its own line.
        if (l.elementType === E.PACKAGE_STATEMENT || l.elementType === E.IMPORT_STATEMENT) {
            if (r.elementType in COMMENT_TOKENS) {
                return Spacing.createSpacing(1, 1, 0, true, common.KEEP_BLANK_LINES_IN_DECLARATIONS)
            }
            val blank = when {
                l.elementType === E.PACKAGE_STATEMENT && r.elementType === E.IMPORT_STATEMENT ->
                    maxOf(common.BLANK_LINES_AFTER_PACKAGE, common.BLANK_LINES_BEFORE_IMPORTS)
                l.elementType === E.PACKAGE_STATEMENT -> common.BLANK_LINES_AFTER_PACKAGE
                r.elementType === E.IMPORT_STATEMENT -> 0
                else -> common.BLANK_LINES_AFTER_IMPORTS
            }
            return blankLines(blank, common.KEEP_BLANK_LINES_IN_DECLARATIONS)
        }

        // The edges of a body: "After class header", "Before class end",
        // "Before method body", "Keep blank lines before '}'", and the
        // "Keep simple ... in one line" options.
        if (p === E.CLASS_BODY || p === E.CODE_BLOCK) edgeSpacing(parent.node, l, r, ctx)?.let { return it }

        // Between declarations: "Around class", "Around method", "Around
        // field" (and their interface variants), where the two are already
        // on separate lines; a one-liner `interface A { a(); b(); }` stays.
        if ((p === E.CLASS_BODY || p === dev.jux.intellij.psi.JUX_FILE) && hasLineBreak(l, r)) {
            val min = minBlankLinesBetween(parent.node, l, r, common)
            if (min > 0) return blankLines(min, common.KEEP_BLANK_LINES_IN_DECLARATIONS)
        }

        // Blank-line clamping between members / statements. Punctuation pairs
        // (commas between enum constants, semicolons) stay with the builder.
        if (isClampablePair(l.elementType, r.elementType)) {
            // Inside a one-line body that is not kept on one line, every
            // member or statement takes a line of its own.
            val split = (p === E.CLASS_BODY || p === E.CODE_BLOCK) && splitsOneLiner(parent.node, common)
            if (p === E.CLASS_BODY) {
                return if (split) blankLines(0, common.KEEP_BLANK_LINES_IN_DECLARATIONS)
                else Spacing.createSpacing(1, 1, 0, true, common.KEEP_BLANK_LINES_IN_DECLARATIONS)
            }
            if (p === E.CODE_BLOCK) {
                return if (split) blankLines(0, common.KEEP_BLANK_LINES_IN_CODE)
                else Spacing.createSpacing(1, 1, 0, true, common.KEEP_BLANK_LINES_IN_CODE)
            }
        }
        return null
    }

    /**
     * At least [min] blank lines between two lines, and at most [keep] (or
     * [min], when more): the shape of every "Minimum blank lines" option.
     */
    private fun blankLines(min: Int, keep: Int): Spacing =
        Spacing.createSpacing(0, 0, min + 1, true, maxOf(keep, min))

    /** Whether the source already has a line break between [l] and [r]. */
    private fun hasLineBreak(l: ASTNode, r: ASTNode): Boolean {
        var n = l.treeNext
        while (n != null && n !== r) {
            if (n.elementType === TokenType.WHITE_SPACE && n.textContains('\n')) return true
            n = n.treeNext
        }
        return false
    }

    /**
     * The "... on new line" option for `else`, `catch`, `finally` and the
     * `while` of `do ... while`, or null when [r] is none of them.
     */
    private fun cuddledKeywordOption(p: IElementType, r: ASTNode, common: CommonCodeStyleSettings): Boolean? = when {
        p === E.IF_STATEMENT && r.elementType === T.ELSE_KW -> common.ELSE_ON_NEW_LINE
        p === E.TRY_STATEMENT && r.elementType === E.CATCH_CLAUSE -> common.CATCH_ON_NEW_LINE
        p === E.TRY_STATEMENT && r.elementType === E.FINALLY_CLAUSE -> common.FINALLY_ON_NEW_LINE
        p === E.DO_WHILE_STATEMENT && r.elementType === T.WHILE_KW -> common.WHILE_ON_NEW_LINE
        else -> null
    }

    // ---- braces placement ----------------------------------------------------

    /**
     * The spacing before a body's `{` under its brace style: a type body
     * (class), a method, constructor, operator or accessor body (method), a
     * lambda body (lambda), and every other statement body (others). A block
     * that stands alone as a statement or a switch arm is left as it is.
     */
    private fun braceSpacing(parent: ASTNode, l: ASTNode, r: ASTNode, ctx: JuxFormatContext): Spacing? {
        val p = parent.elementType
        val style = when {
            r.elementType === E.CLASS_BODY -> ctx.jux.CLASS_BRACE_STYLE
            r.elementType === E.CODE_BLOCK -> when (p) {
                in METHOD_LIKE -> ctx.jux.METHOD_BRACE_STYLE
                E.LAMBDA_EXPRESSION -> ctx.jux.LAMBDA_BRACE_STYLE
                in STATEMENT_OWNERS -> ctx.jux.BRACE_STYLE
                else -> return null
            }
            r.elementType === T.LBRACE && (p === E.SWITCH_STATEMENT || p === E.SWITCH_EXPRESSION) -> ctx.jux.BRACE_STYLE
            else -> return null
        }
        // `if (x)` over two lines is a wrapped header; so is a signature.
        val nextLine = when (style) {
            JuxCodeStyleSettings.NEXT_LINE -> true
            JuxCodeStyleSettings.NEXT_LINE_IF_WRAPPED -> headerIsWrapped(parent, r)
            else -> false
        }
        return if (nextLine) Spacing.createSpacing(0, 0, 1, false, 0)
        else Spacing.createSpacing(1, 1, 0, false, 0)
    }

    /**
     * Whether the header before [brace] spans several lines, not counting the
     * annotations, modifiers and comments on lines of their own above it.
     */
    private fun headerIsWrapped(construct: ASTNode, brace: ASTNode): Boolean {
        var start: ASTNode? = construct.firstChildNode
        while (start != null && (start.elementType === TokenType.WHITE_SPACE || start.elementType in COMMENT_TOKENS ||
                start.elementType === E.ANNOTATION || start.elementType === E.MODIFIER_LIST)
        ) {
            start = start.treeNext
        }
        start ?: return false
        var n: ASTNode? = start
        while (n != null && n !== brace) {
            if (n.textContains('\n')) return true
            n = n.treeNext
        }
        return false
    }

    // ---- the edges of a body -------------------------------------------------

    /** Spacing right after a body's `{` and right before its `}`. */
    private fun edgeSpacing(body: ASTNode, l: ASTNode, r: ASTNode, ctx: JuxFormatContext): Spacing? {
        val common = ctx.common
        val isClass = body.elementType === E.CLASS_BODY
        // An empty body reads `{}`, as Java writes it; `{\n}` keeps its break.
        if (l.elementType === T.LBRACE && r.elementType === T.RBRACE) return Spacing.createSpacing(0, 0, 0, true, 0)
        val afterOpen = l.elementType === T.LBRACE && r.elementType !== T.RBRACE
        val beforeClose = r.elementType === T.RBRACE && l.elementType !== T.LBRACE
        if (!afterOpen && !beforeClose) return null
        // A one-line body that is not to be kept so opens onto its own lines.
        if (splitsOneLiner(body, common)) {
            return blankLines(0, if (isClass) common.KEEP_BLANK_LINES_IN_DECLARATIONS else common.KEEP_BLANK_LINES_IN_CODE)
        }
        if (!hasLineBreak(l, r)) return null
        return when {
            afterOpen && isClass -> blankLines(common.BLANK_LINES_AFTER_CLASS_HEADER, common.KEEP_BLANK_LINES_IN_DECLARATIONS)
            afterOpen && body.treeParent?.elementType in METHOD_LIKE ->
                blankLines(common.BLANK_LINES_BEFORE_METHOD_BODY, common.KEEP_BLANK_LINES_IN_CODE)
            beforeClose && isClass -> blankLines(common.BLANK_LINES_BEFORE_CLASS_END, common.KEEP_BLANK_LINES_BEFORE_RBRACE)
            beforeClose -> blankLines(0, common.KEEP_BLANK_LINES_BEFORE_RBRACE)
            else -> null
        }
    }

    /**
     * Whether [body] is written on one line and its "Keep simple ... in one
     * line" option says to open it up: classes, methods (and accessors),
     * lambdas, and every other block. An empty `{}` is left alone.
     */
    private fun splitsOneLiner(body: ASTNode, common: CommonCodeStyleSettings): Boolean {
        if (body.textContains('\n')) return false
        if (body.getChildren(null).none { it.elementType !== TokenType.WHITE_SPACE && it.elementType !== T.LBRACE && it.elementType !== T.RBRACE }) {
            return false
        }
        val keep = when {
            body.elementType === E.CLASS_BODY -> common.KEEP_SIMPLE_CLASSES_IN_ONE_LINE
            body.treeParent?.elementType in METHOD_LIKE -> common.KEEP_SIMPLE_METHODS_IN_ONE_LINE
            body.treeParent?.elementType === E.LAMBDA_EXPRESSION -> common.KEEP_SIMPLE_LAMBDAS_IN_ONE_LINE
            else -> common.KEEP_SIMPLE_BLOCKS_IN_ONE_LINE
        }
        return !keep
    }

    // ---- blank lines between declarations -----------------------------------

    /** What a declaration counts as for the "Around ..." options. */
    private enum class DeclKind { CLASS, METHOD, FIELD }

    private fun declKind(node: ASTNode): DeclKind? = when (node.elementType) {
        in TYPE_DECLARATIONS -> DeclKind.CLASS
        in METHOD_LIKE, E.INIT_BLOCK, E.STATIC_BLOCK, E.DROP_BLOCK -> DeclKind.METHOD
        E.FIELD_DECLARATION, E.CONST_DECLARATION, E.PROPERTY_DECLARATION -> DeclKind.FIELD
        else -> null
    }

    /**
     * The fewest blank lines between declarations [l] and [r] in [container]
     * (a class body or the file): the largest of the options that apply,
     * with an interface's own "... in interface" variants.
     */
    private fun minBlankLinesBetween(container: ASTNode, l: ASTNode, r: ASTNode, common: CommonCodeStyleSettings): Int {
        val kinds = setOf(declKind(l) ?: return 0, declKind(r) ?: return 0)
        val inInterface = container.treeParent?.elementType === E.INTERFACE_DECLARATION
        var min = 0
        if (DeclKind.CLASS in kinds) min = maxOf(min, common.BLANK_LINES_AROUND_CLASS)
        if (DeclKind.METHOD in kinds) {
            min = maxOf(min, if (inInterface) common.BLANK_LINES_AROUND_METHOD_IN_INTERFACE else common.BLANK_LINES_AROUND_METHOD)
        }
        if (DeclKind.FIELD in kinds) {
            min = maxOf(min, if (inInterface) common.BLANK_LINES_AROUND_FIELD_IN_INTERFACE else common.BLANK_LINES_AROUND_FIELD)
        }
        return min
    }

    private fun isClampablePair(l: com.intellij.psi.tree.IElementType, r: com.intellij.psi.tree.IElementType): Boolean =
        l !== T.LBRACE && r !== T.RBRACE &&
            l !== T.COMMA && r !== T.COMMA &&
            l !== T.SEMICOLON && r !== T.SEMICOLON

    // ---- node groups ---------------------------------------------------------

    private val COMMENT_TOKENS = setOf(T.LINE_COMMENT, T.BLOCK_COMMENT, T.DOC_COMMENT)

    /** Declarations whose body follows the method brace style. */
    private val METHOD_LIKE = setOf(
        E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION, E.PROPERTY_ACCESSOR,
    )

    /** Statements whose block body follows the "Others" brace style. */
    private val STATEMENT_OWNERS = setOf(
        E.IF_STATEMENT, E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT, E.FOR_STATEMENT, E.FOR_EACH_STATEMENT,
        E.TRY_STATEMENT, E.CATCH_CLAUSE, E.FINALLY_CLAUSE, E.LABELED_STATEMENT, E.UNSAFE_STATEMENT,
        E.INIT_BLOCK, E.STATIC_BLOCK, E.DROP_BLOCK,
    )

    private val TYPE_DECLARATIONS = setOf(
        E.CLASS_DECLARATION, E.INTERFACE_DECLARATION, E.ENUM_DECLARATION, E.RECORD_DECLARATION,
        E.STRUCT_DECLARATION, E.ANNOTATION_DECLARATION,
    )

    // ---- operator groups (mirrors §A.4 / the expression parser) ------------

    private val GT_CLOSERS = TokenSet.create(T.GT, T.GT_GT)
    private val UNARY_OPS = TokenSet.create(
        T.BANG, T.TILDE, T.MINUS, T.PLUS, T.AMP, T.STAR, T.PLUS_PLUS, T.MINUS_MINUS,
    )
    /** `++` / `--`: hug their operand on either side (`i++`, `--i`). */
    private val INC_DEC_OPS = TokenSet.create(T.PLUS_PLUS, T.MINUS_MINUS)
    private val ASSIGN_OPS = TokenSet.create(
        T.EQ, T.PLUS_EQ, T.MINUS_EQ, T.STAR_EQ, T.SLASH_EQ, T.PERCENT_EQ,
        T.AMP_EQ, T.PIPE_EQ, T.CARET_EQ, T.LT_LT_EQ, T.GT_GT_EQ,
    )
    private val LOGICAL_OPS = TokenSet.create(T.AND_AND, T.OR_OR)
    private val EQUALITY_OPS = TokenSet.create(T.EQ_EQ, T.NOT_EQ, T.STRICT_EQ, T.STRICT_NOT_EQ)
    private val RELATIONAL_OPS = TokenSet.create(T.LT, T.LE, T.GT, T.GE)
    // Wrapping variants (`+%`, `<<%`, …) space exactly like their base ops.
    private val SHIFT_OPS = TokenSet.create(T.LT_LT, T.GT_GT, T.LT_LT_PERCENT, T.GT_GT_PERCENT)
    private val ADDITIVE_OPS = TokenSet.create(T.PLUS, T.MINUS, T.PLUS_PERCENT, T.MINUS_PERCENT)
    private val MULTIPLICATIVE_OPS = TokenSet.create(T.STAR, T.SLASH, T.PERCENT, T.STAR_PERCENT)
    private val BITWISE_OPS = TokenSet.create(T.AMP, T.PIPE, T.CARET)
    private val ELVIS_OPS = TokenSet.create(T.QUESTION_COLON, T.QUESTION_QUESTION)

    /** Keywords that always read with one space before what follows. */
    private val KEYWORDS_THEN_SPACE = TokenSet.create(
        T.IF_KW, T.ELSE_KW, T.WHILE_KW, T.FOR_KW, T.DO_KW, T.SWITCH_KW,
        T.CATCH_KW, T.TRY_KW, T.FINALLY_KW, T.RETURN_KW, T.THROW_KW,
        T.NEW_KW, T.CASE_KW, T.YIELD_KW, T.WHEN_KW,
    )

    /** Clause keywords spaced on both sides (`extends`, `as`, …). */
    private val CLAUSE_KEYWORDS = TokenSet.create(
        T.EXTENDS_KW, T.IMPLEMENTS_KW, T.PERMITS_KW, T.THROWS_KW, T.AS_KW,
    )
}
