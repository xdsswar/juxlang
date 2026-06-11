package dev.jux.intellij.format

import com.intellij.formatting.Block
import com.intellij.formatting.ChildAttributes
import com.intellij.formatting.Indent
import com.intellij.formatting.Spacing
import com.intellij.lang.ASTNode
import com.intellij.psi.TokenType
import com.intellij.psi.formatter.common.AbstractBlock
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * The single, universal formatting block: indentation comes from the
 * [JuxIndentRules] table, spacing from [JuxSpacingRules]. The grammar has
 * ~60 composite kinds but only a handful of distinct formatting behaviours,
 * so per-kind block classes (the `AbstractJavaBlock` family) would be pure
 * overhead for the v1 preserve-line-breaks policy — no Wrap or Alignment
 * objects exist anywhere in this tree.
 */
class JuxBlock(
    node: ASTNode,
    private val indent: Indent,
    private val ctx: JuxFormatContext,
) : AbstractBlock(node, /* wrap = */ null, /* alignment = */ null) {

    override fun getIndent(): Indent = indent

    override fun buildChildren(): List<Block> {
        if (isLeaf) return emptyList()
        val out = ArrayList<Block>()
        var child = myNode.firstChildNode
        while (child != null) {
            // Skip whitespace AND zero-length nodes: the parser's recovery
            // emits zero-width PsiErrorElements everywhere, and the engine
            // asserts on empty-range blocks.
            if (child.elementType !== TokenType.WHITE_SPACE && child.textLength > 0) {
                out.add(JuxBlock(child, JuxIndentRules.childIndent(myNode, child), ctx))
            }
            child = child.treeNext
        }
        return out
    }

    override fun getSpacing(child1: Block?, child2: Block): Spacing? =
        JuxSpacingRules.custom(this, child1, child2, ctx)
            ?: ctx.spacingBuilder.getSpacing(this, child1, child2)

    override fun getChildAttributes(newChildIndex: Int): ChildAttributes =
        ChildAttributes(JuxIndentRules.newChildIndent(myNode), null)

    /**
     * Tokens are leaves; so are the parser's **opaque balanced runs** — raw
     * multi-line token soup whose interior layout must not be rewritten
     * (annotation arguments, record components, anonymous-class bodies,
     * `where` constraint runs). String literals (incl. interpolated) are
     * single lexer tokens, so their interiors are untouchable by construction.
     */
    override fun isLeaf(): Boolean = myNode.firstChildNode == null || isOpaque(myNode)

    private fun isOpaque(node: ASTNode): Boolean = when (node.elementType) {
        E.ANNOTATION, E.RECORD_COMPONENT_LIST, E.WHERE_CLAUSE -> true
        // `new T(…) { … }` / `new T[…] { … }`: the brace run is raw tokens
        // consumed by skipMatched — re-indenting it would scramble user code.
        E.NEW_EXPRESSION -> node.findChildByType(T.LBRACE) != null
        else -> false
    }
}
