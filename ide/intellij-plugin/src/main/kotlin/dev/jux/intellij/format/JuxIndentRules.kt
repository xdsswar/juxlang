package dev.jux.intellij.format

import com.intellij.formatting.Indent
import com.intellij.lang.ASTNode
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * The formatter's indentation policy as two pure dispatch tables — kept in one
 * file so the whole policy is reviewable on a screen and unit-testable without
 * platform machinery.
 *
 * The shape mirrors the language's K&R style: 4-space members/statements
 * (NORMAL), 8-space wrapped expressions and argument lists (CONTINUATION),
 * and closing braces / `else` / `catch` / `finally` aligned with their
 * construct (NONE).
 */
object JuxIndentRules {

    /** Indent for [child] inside [parent] — the table the block tree applies. */
    fun childIndent(parent: ASTNode, child: ASTNode): Indent {
        val p = parent.elementType
        val c = child.elementType

        // Braces always align with the construct's first line.
        if (c === T.LBRACE || c === T.RBRACE) return Indent.getNoneIndent()

        return when (p) {
            // ---- brace bodies: members / statements / enum constants -------
            E.CLASS_BODY, E.CODE_BLOCK, E.PROPERTY_ACCESSOR_LIST -> Indent.getNormalIndent()

            // §L.7 native block: unlike CLASS_BODY this node also holds the
            // header (`@extern` annotation + `unsafe native` modifiers), which
            // must stay at the block's own column — only the foreign-fn
            // declarations between the braces indent one level.
            E.EXTERN_BLOCK ->
                if (c === E.METHOD_DECLARATION || c in COMMENTS) Indent.getNormalIndent()
                else Indent.getNoneIndent()

            // `case …` arms sit one level inside `switch {`.
            E.SWITCH_STATEMENT, E.SWITCH_EXPRESSION ->
                if (c === E.SWITCH_CASE || c in COMMENTS) Indent.getNormalIndent()
                else Indent.getNoneIndent()

            // Single-statement bodies: `if (x)\n    foo();` — the statement
            // child (not a block; blocks indent their own children) gets +1.
            E.IF_STATEMENT, E.WHILE_STATEMENT, E.FOR_STATEMENT,
            E.FOR_EACH_STATEMENT, E.DO_WHILE_STATEMENT, E.LABELED_STATEMENT ->
                if (c in STATEMENTS && c !== E.CODE_BLOCK) Indent.getNormalIndent()
                else Indent.getNoneIndent()

            // ---- wrapped expressions: continuation, first child anchors ----
            // `continuationWithoutFirst` makes left-deep nests (chained calls,
            // long binary expressions) indent +8 once instead of stacking.
            E.BINARY_EXPRESSION, E.ASSIGNMENT_EXPRESSION, E.CONDITIONAL_EXPRESSION,
            E.RANGE_EXPRESSION, E.CAST_EXPRESSION,
            E.CALL_EXPRESSION, E.FIELD_ACCESS_EXPRESSION, E.INDEX_EXPRESSION,
            E.METHOD_REF_EXPRESSION, E.POSTFIX_EXPRESSION,
            E.EXTENDS_CLAUSE, E.IMPLEMENTS_CLAUSE, E.PERMITS_CLAUSE, E.THROWS_CLAUSE,
            E.FIELD_DECLARATION, E.PROPERTY_DECLARATION, E.LOCAL_VARIABLE,
            E.EXPRESSION_STATEMENT, E.PROPERTY_ACCESSOR,
            E.RETURN_STATEMENT, E.THROW_STATEMENT, E.LAMBDA_EXPRESSION, E.SWITCH_CASE ->
                when (c) {
                    // Bodies indent themselves; annotations/modifiers align
                    // with the declaration they precede.
                    E.CODE_BLOCK, E.CLASS_BODY, E.PROPERTY_ACCESSOR_LIST,
                    E.ANNOTATION, E.MODIFIER_LIST ->
                        Indent.getNoneIndent()
                    else -> Indent.getContinuationWithoutFirstIndent()
                }

            // ---- delimited lists: contents continuation, delimiters anchor --
            E.PARAMETER_LIST, E.ARGUMENT_LIST, E.TYPE_PARAMETER_LIST,
            E.TYPE_ARGUMENT_LIST, E.ANNOTATION_ARGUMENT_LIST ->
                if (c === T.LPAREN || c === T.RPAREN || c === T.LT ||
                    c === T.GT || c === T.GT_GT
                ) Indent.getNoneIndent()
                else Indent.getContinuationIndent()

            else -> Indent.getNoneIndent()
        }
    }

    /** Indent for a NEW child typed on Enter — `getChildAttributes` mirror. */
    fun newChildIndent(parent: ASTNode): Indent = when (parent.elementType) {
        E.CLASS_BODY, E.CODE_BLOCK, E.PROPERTY_ACCESSOR_LIST, E.EXTERN_BLOCK,
        E.SWITCH_STATEMENT, E.SWITCH_EXPRESSION -> Indent.getNormalIndent()
        E.PARAMETER_LIST, E.ARGUMENT_LIST, E.TYPE_PARAMETER_LIST,
        E.TYPE_ARGUMENT_LIST, E.ANNOTATION_ARGUMENT_LIST,
        E.BINARY_EXPRESSION, E.ASSIGNMENT_EXPRESSION, E.CONDITIONAL_EXPRESSION,
        E.CALL_EXPRESSION, E.FIELD_ACCESS_EXPRESSION -> Indent.getContinuationIndent()
        else -> Indent.getNoneIndent()
    }

    private val COMMENTS = setOf(T.LINE_COMMENT, T.BLOCK_COMMENT, T.DOC_COMMENT)

    /** Statement-level element types (for single-statement loop/if bodies). */
    private val STATEMENTS = setOf(
        E.EXPRESSION_STATEMENT, E.LOCAL_VARIABLE, E.IF_STATEMENT,
        E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT, E.FOR_STATEMENT,
        E.FOR_EACH_STATEMENT, E.SWITCH_STATEMENT, E.RETURN_STATEMENT,
        E.BREAK_STATEMENT, E.CONTINUE_STATEMENT, E.THROW_STATEMENT,
        E.TRY_STATEMENT, E.UNSAFE_STATEMENT, E.LABELED_STATEMENT,
        E.EMPTY_STATEMENT,
    )
}
