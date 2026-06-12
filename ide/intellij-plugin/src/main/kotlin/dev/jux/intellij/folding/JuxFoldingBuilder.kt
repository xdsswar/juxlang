package dev.jux.intellij.folding

import com.intellij.lang.ASTNode
import com.intellij.lang.folding.FoldingBuilderEx
import com.intellij.lang.folding.FoldingDescriptor
import com.intellij.openapi.editor.Document
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Code folding from the PSI tree: class bodies, method/initializer blocks,
 * switch bodies, multi-line raw strings, and multi-line block / doc comments
 * collapse. Driven entirely off node element types, so it stays in lockstep
 * with the parser.
 */
class JuxFoldingBuilder : FoldingBuilderEx(), DumbAware {

    override fun buildFoldRegions(root: PsiElement, document: Document, quick: Boolean): Array<FoldingDescriptor> {
        val out = ArrayList<FoldingDescriptor>()
        collect(root.node, document, out)
        return out.toTypedArray()
    }

    private fun collect(node: ASTNode, document: Document, out: MutableList<FoldingDescriptor>) {
        val type = node.elementType
        when {
            type in FOLDABLE && isMultiline(node.textRange, document) ->
                out.add(FoldingDescriptor(node, node.textRange))
            // A switch's case list isn't a CODE_BLOCK, so fold it explicitly —
            // from its `{` (keeping `switch (expr)` visible) to the node end.
            type in SWITCHES -> braceRange(node)?.let { range ->
                if (isMultiline(range, document)) out.add(FoldingDescriptor(node, range))
            }
            // Multi-line raw strings (`"""…"""` / `$"""…"""`) collapse to
            // their first line's worth of meaning.
            type in RAW_STRINGS && isMultiline(node.textRange, document) ->
                out.add(FoldingDescriptor(node, node.textRange))
        }
        var child = node.firstChildNode
        while (child != null) {
            collect(child, document, out)
            child = child.treeNext
        }
    }

    /** The range from the node's first `{` child to the node end, or null. */
    private fun braceRange(node: ASTNode): TextRange? {
        var child = node.firstChildNode
        while (child != null) {
            if (child.elementType === T.LBRACE) {
                return TextRange(child.startOffset, node.textRange.endOffset)
            }
            child = child.treeNext
        }
        return null
    }

    private fun isMultiline(range: TextRange, document: Document): Boolean =
        range.endOffset <= document.textLength &&
            document.getLineNumber(range.startOffset) < document.getLineNumber(range.endOffset)

    override fun getPlaceholderText(node: ASTNode): String = when (node.elementType) {
        T.BLOCK_COMMENT, T.DOC_COMMENT -> "/*...*/"
        T.RAW_STRING_LITERAL, T.INTERP_RAW_STRING_LITERAL -> "\"\"\"...\"\"\""
        else -> "{...}"
    }

    override fun isCollapsedByDefault(node: ASTNode): Boolean = false

    private companion object {
        val FOLDABLE = setOf(
            E.CLASS_BODY, E.CODE_BLOCK, E.PROPERTY_ACCESSOR_LIST,
            T.BLOCK_COMMENT, T.DOC_COMMENT,
        )
        val SWITCHES = setOf(E.SWITCH_STATEMENT, E.SWITCH_EXPRESSION)
        val RAW_STRINGS = setOf(T.RAW_STRING_LITERAL, T.INTERP_RAW_STRING_LITERAL)
    }
}
