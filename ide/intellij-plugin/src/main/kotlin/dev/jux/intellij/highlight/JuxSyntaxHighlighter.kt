package dev.jux.intellij.highlight

import com.intellij.lexer.Lexer
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors as D
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.editor.colors.TextAttributesKey.createTextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase
import com.intellij.psi.tree.IElementType

/**
 * Maps [JuxLexer] tokens to colour attribute keys. Each key inherits from a
 * `DefaultLanguageHighlighterColors` base, so Jux follows the active theme out
 * of the box and stays customizable in **Settings → Editor → Color Scheme →
 * Jux** (see [JuxColorSettingsPage]).
 */
class JuxSyntaxHighlighter : SyntaxHighlighterBase() {
    override fun getHighlightingLexer(): Lexer = JuxLexer()

    override fun getTokenHighlights(tokenType: IElementType): Array<TextAttributesKey> =
        KEYS[tokenType]?.let { arrayOf(it) } ?: EMPTY

    companion object {
        val KEYWORD = key("JUX_KEYWORD", D.KEYWORD)
        val TYPE = key("JUX_TYPE", D.KEYWORD)
        val CONSTANT = key("JUX_CONSTANT", D.KEYWORD)
        val STRING = key("JUX_STRING", D.STRING)
        val CHAR = key("JUX_CHAR", D.STRING)
        val NUMBER = key("JUX_NUMBER", D.NUMBER)
        val LINE_COMMENT = key("JUX_LINE_COMMENT", D.LINE_COMMENT)
        val BLOCK_COMMENT = key("JUX_BLOCK_COMMENT", D.BLOCK_COMMENT)
        val DOC_COMMENT = key("JUX_DOC_COMMENT", D.DOC_COMMENT)
        val ANNOTATION = key("JUX_ANNOTATION", D.METADATA)
        val OPERATOR = key("JUX_OPERATOR", D.OPERATION_SIGN)
        val BRACES = key("JUX_BRACES", D.BRACES)
        val BRACKETS = key("JUX_BRACKETS", D.BRACKETS)
        val PARENS = key("JUX_PARENS", D.PARENTHESES)
        val SEMICOLON = key("JUX_SEMICOLON", D.SEMICOLON)
        val COMMA = key("JUX_COMMA", D.COMMA)
        val DOT = key("JUX_DOT", D.DOT)
        val IDENTIFIER = key("JUX_IDENTIFIER", D.IDENTIFIER)

        private val EMPTY = emptyArray<TextAttributesKey>()

        private fun key(externalName: String, base: TextAttributesKey) =
            createTextAttributesKey(externalName, base)

        private val KEYS: Map<IElementType, TextAttributesKey> = mapOf(
            JuxTokenTypes.KEYWORD to KEYWORD,
            JuxTokenTypes.TYPE to TYPE,
            JuxTokenTypes.CONSTANT to CONSTANT,
            JuxTokenTypes.STRING to STRING,
            JuxTokenTypes.CHAR to CHAR,
            JuxTokenTypes.NUMBER to NUMBER,
            JuxTokenTypes.LINE_COMMENT to LINE_COMMENT,
            JuxTokenTypes.BLOCK_COMMENT to BLOCK_COMMENT,
            JuxTokenTypes.DOC_COMMENT to DOC_COMMENT,
            JuxTokenTypes.ANNOTATION to ANNOTATION,
            JuxTokenTypes.OPERATOR to OPERATOR,
            JuxTokenTypes.LBRACE to BRACES,
            JuxTokenTypes.RBRACE to BRACES,
            JuxTokenTypes.LBRACKET to BRACKETS,
            JuxTokenTypes.RBRACKET to BRACKETS,
            JuxTokenTypes.LPAREN to PARENS,
            JuxTokenTypes.RPAREN to PARENS,
            JuxTokenTypes.SEMICOLON to SEMICOLON,
            JuxTokenTypes.COMMA to COMMA,
            JuxTokenTypes.DOT to DOT,
            JuxTokenTypes.IDENTIFIER to IDENTIFIER,
        )
    }
}
