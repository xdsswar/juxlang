package dev.jux.intellij.format

import com.intellij.formatting.Block
import com.intellij.formatting.Spacing
import com.intellij.formatting.SpacingBuilder
import com.intellij.psi.codeStyle.CodeStyleSettings
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
 * v1 wrap policy: **preserve the user's line breaks** — `spaces(n)` rules
 * normalize same-line spacing only; nothing joins or splits lines except the
 * package/import separator. The examples corpus has deliberate one-liner
 * interfaces that must survive reformat.
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

        // Cuddled keywords: `} else {`, `} catch`, `} finally`, `} while` —
        // one space when on the same line, but a user's next-line style survives.
        if ((p === E.IF_STATEMENT && r.elementType === T.ELSE_KW) ||
            (p === E.TRY_STATEMENT &&
                (r.elementType === E.CATCH_CLAUSE || r.elementType === E.FINALLY_CLAUSE)) ||
            (p === E.DO_WHILE_STATEMENT && r.elementType === T.WHILE_KW)
        ) {
            return Spacing.createSpacing(1, 1, 0, true, 0)
        }

        // Headers: exactly one line break after `package` and each `import`
        // (the only place v1 forces a newline), keeping intentional blanks.
        // A trailing same-line comment (`import a.b.C; // why`) is exempt —
        // forcing the break would tear the comment onto its own line.
        if (l.elementType === E.PACKAGE_STATEMENT || l.elementType === E.IMPORT_STATEMENT) {
            if (r.elementType === T.LINE_COMMENT || r.elementType === T.BLOCK_COMMENT ||
                r.elementType === T.DOC_COMMENT
            ) {
                return Spacing.createSpacing(1, 1, 0, true, ctx.common.KEEP_BLANK_LINES_IN_DECLARATIONS)
            }
            return Spacing.createSpacing(
                0, 0, 1, true, ctx.common.KEEP_BLANK_LINES_IN_DECLARATIONS,
            )
        }

        // Blank-line clamping between members / statements. Punctuation pairs
        // (commas between enum constants, semicolons) stay with the builder.
        if (isClampablePair(l.elementType, r.elementType)) {
            if (p === E.CLASS_BODY) {
                return Spacing.createSpacing(
                    1, 1, 0, true, ctx.common.KEEP_BLANK_LINES_IN_DECLARATIONS,
                )
            }
            if (p === E.CODE_BLOCK) {
                return Spacing.createSpacing(
                    1, 1, 0, true, ctx.common.KEEP_BLANK_LINES_IN_CODE,
                )
            }
        }
        return null
    }

    private fun isClampablePair(l: com.intellij.psi.tree.IElementType, r: com.intellij.psi.tree.IElementType): Boolean =
        l !== T.LBRACE && r !== T.RBRACE &&
            l !== T.COMMA && r !== T.COMMA &&
            l !== T.SEMICOLON && r !== T.SEMICOLON

    // ---- operator groups (mirrors §A.4 / the expression parser) ------------

    private val GT_CLOSERS = TokenSet.create(T.GT, T.GT_GT)
    private val UNARY_OPS = TokenSet.create(T.BANG, T.TILDE, T.MINUS, T.PLUS, T.AMP, T.STAR)
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
