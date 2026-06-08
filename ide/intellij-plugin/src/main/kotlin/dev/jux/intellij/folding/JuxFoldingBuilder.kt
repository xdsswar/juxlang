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
 * Code folding from the PSI tree: class bodies, method/initializer blocks, and
 * multi-line block / doc comments collapse. Driven entirely off node element
 * types, so it stays in lockstep with the parser.
 */
class JuxFoldingBuilder : FoldingBuilderEx(), DumbAware {

    override fun buildFoldRegions(root: PsiElement, document: Document, quick: Boolean): Array<FoldingDescriptor> {
        val out = ArrayList<FoldingDescriptor>()
        collect(root.node, document, out)
        return out.toTypedArray()
    }

    private fun collect(node: ASTNode, document: Document, out: MutableList<FoldingDescriptor>) {
        if (node.elementType in FOLDABLE && isMultiline(node.textRange, document)) {
            out.add(FoldingDescriptor(node, node.textRange))
        }
        var child = node.firstChildNode
        while (child != null) {
            collect(child, document, out)
            child = child.treeNext
        }
    }

    private fun isMultiline(range: TextRange, document: Document): Boolean =
        range.endOffset <= document.textLength &&
            document.getLineNumber(range.startOffset) < document.getLineNumber(range.endOffset)

    override fun getPlaceholderText(node: ASTNode): String = when (node.elementType) {
        T.BLOCK_COMMENT, T.DOC_COMMENT -> "/*...*/"
        else -> "{...}"
    }

    override fun isCollapsedByDefault(node: ASTNode): Boolean = false

    private companion object {
        val FOLDABLE = setOf(E.CLASS_BODY, E.CODE_BLOCK, T.BLOCK_COMMENT, T.DOC_COMMENT)
    }
}
