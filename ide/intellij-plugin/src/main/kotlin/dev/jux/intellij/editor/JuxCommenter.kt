package dev.jux.intellij.editor

import com.intellij.lang.CodeDocumentationAwareCommenter
import com.intellij.psi.PsiComment
import com.intellij.psi.tree.IElementType
import dev.jux.intellij.highlight.JuxTokenTypes

/**
 * Comment support: enables `Ctrl+/` (line comment) and `Ctrl+Shift+/` (block
 * comment) toggling, and declares the doc-comment shape (`/** * */`) used by
 * the editor.
 */
class JuxCommenter : CodeDocumentationAwareCommenter {
    override fun getLineCommentPrefix(): String = "//"
    override fun getBlockCommentPrefix(): String = "/*"
    override fun getBlockCommentSuffix(): String = "*/"
    override fun getCommentedBlockCommentPrefix(): String? = null
    override fun getCommentedBlockCommentSuffix(): String? = null

    override fun getDocumentationCommentPrefix(): String = "/**"
    override fun getDocumentationCommentLinePrefix(): String = "*"
    override fun getDocumentationCommentSuffix(): String = "*/"
    override fun isDocumentationComment(element: PsiComment?): Boolean = false

    override fun getLineCommentTokenType(): IElementType = JuxTokenTypes.LINE_COMMENT
    override fun getBlockCommentTokenType(): IElementType = JuxTokenTypes.BLOCK_COMMENT
    override fun getDocumentationCommentTokenType(): IElementType = JuxTokenTypes.DOC_COMMENT
}
