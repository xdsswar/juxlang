package dev.jux.intellij.editor

import com.intellij.lang.Language
import com.intellij.psi.TokenType
import com.intellij.psi.impl.source.codeStyle.SemanticEditorPosition.SyntaxElement
import com.intellij.psi.impl.source.codeStyle.lineIndent.JavaLikeLangLineIndentProvider
import com.intellij.psi.impl.source.codeStyle.lineIndent.JavaLikeLangLineIndentProvider.JavaLikeElement
import com.intellij.psi.tree.IElementType
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.highlight.JuxTokenTypes

/**
 * Java-like smart indentation without a full formatter (no PSI yet).
 *
 * It maps Jux lexer tokens to the platform's "Java-like" indentation elements,
 * so pressing Enter inside a `{ … }` block lands the caret at the correct
 * indent, `}` lines up with its opener, parentheses/brackets and `;` behave as
 * in Java, and **Auto-Indent Lines** (`Ctrl+Alt+I`) reflows to match. Full
 * reformat (`Ctrl+Alt+L`) and Optimize Imports still need declaration-level PSI.
 */
class JuxLineIndentProvider : JavaLikeLangLineIndentProvider() {
    override fun isSuitableForLanguage(language: Language): Boolean = language is JuxLanguage

    override fun mapType(tokenType: IElementType): SyntaxElement? = MAP[tokenType]

    private companion object {
        val MAP: Map<IElementType, SyntaxElement> = mapOf(
            TokenType.WHITE_SPACE to JavaLikeElement.Whitespace,
            JuxTokenTypes.SEMICOLON to JavaLikeElement.Semicolon,
            JuxTokenTypes.LBRACE to JavaLikeElement.BlockOpeningBrace,
            JuxTokenTypes.RBRACE to JavaLikeElement.BlockClosingBrace,
            JuxTokenTypes.LBRACKET to JavaLikeElement.ArrayOpeningBracket,
            JuxTokenTypes.RBRACKET to JavaLikeElement.ArrayClosingBracket,
            JuxTokenTypes.LPAREN to JavaLikeElement.LeftParenthesis,
            JuxTokenTypes.RPAREN to JavaLikeElement.RightParenthesis,
            JuxTokenTypes.LINE_COMMENT to JavaLikeElement.LineComment,
            JuxTokenTypes.BLOCK_COMMENT to JavaLikeElement.BlockComment,
            JuxTokenTypes.DOC_COMMENT to JavaLikeElement.BlockComment,
            JuxTokenTypes.COMMA to JavaLikeElement.Comma,
        )
    }
}
