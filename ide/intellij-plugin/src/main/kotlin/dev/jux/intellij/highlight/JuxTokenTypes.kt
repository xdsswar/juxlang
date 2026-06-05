package dev.jux.intellij.highlight

import com.intellij.psi.tree.IElementType
import dev.jux.intellij.JuxLanguage

/**
 * Token types produced by [JuxLexer] for syntax highlighting.
 *
 * This is a lexer-level classification (not a full PSI grammar) — enough to
 * colour every visible construct the way a Java developer expects. Whitespace
 * uses the platform's shared `TokenType.WHITE_SPACE`.
 */
object JuxTokenTypes {
    val LINE_COMMENT = IElementType("JUX_LINE_COMMENT", JuxLanguage)
    val BLOCK_COMMENT = IElementType("JUX_BLOCK_COMMENT", JuxLanguage)
    val DOC_COMMENT = IElementType("JUX_DOC_COMMENT", JuxLanguage)
    val STRING = IElementType("JUX_STRING", JuxLanguage)
    val CHAR = IElementType("JUX_CHAR", JuxLanguage)
    val NUMBER = IElementType("JUX_NUMBER", JuxLanguage)
    val KEYWORD = IElementType("JUX_KEYWORD", JuxLanguage)
    val TYPE = IElementType("JUX_TYPE", JuxLanguage)
    val CONSTANT = IElementType("JUX_CONSTANT", JuxLanguage)
    val IDENTIFIER = IElementType("JUX_IDENTIFIER", JuxLanguage)
    val ANNOTATION = IElementType("JUX_ANNOTATION", JuxLanguage)
    val OPERATOR = IElementType("JUX_OPERATOR", JuxLanguage)
    // Left/right pairs are distinct so the brace matcher can pair them.
    val LBRACE = IElementType("JUX_LBRACE", JuxLanguage)
    val RBRACE = IElementType("JUX_RBRACE", JuxLanguage)
    val LBRACKET = IElementType("JUX_LBRACKET", JuxLanguage)
    val RBRACKET = IElementType("JUX_RBRACKET", JuxLanguage)
    val LPAREN = IElementType("JUX_LPAREN", JuxLanguage)
    val RPAREN = IElementType("JUX_RPAREN", JuxLanguage)
    val SEMICOLON = IElementType("JUX_SEMICOLON", JuxLanguage)
    val COMMA = IElementType("JUX_COMMA", JuxLanguage)
    val DOT = IElementType("JUX_DOT", JuxLanguage)
    val OTHER = IElementType("JUX_OTHER", JuxLanguage)
}
