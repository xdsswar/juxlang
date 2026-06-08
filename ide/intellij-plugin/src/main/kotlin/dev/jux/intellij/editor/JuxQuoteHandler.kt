package dev.jux.intellij.editor

import com.intellij.codeInsight.editorActions.SimpleTokenSetQuoteHandler
import dev.jux.intellij.highlight.JuxTokenTypes

/**
 * Auto-closes `"` and `'`: typing the opening quote inserts the matching close
 * and parks the caret between them, and typing the closing quote over an
 * existing one steps past it (no duplicate). Driven by the lexer's string and
 * char literal token types.
 */
class JuxQuoteHandler : SimpleTokenSetQuoteHandler(
    JuxTokenTypes.STRING_LITERAL,
    JuxTokenTypes.RAW_STRING_LITERAL,
    JuxTokenTypes.INTERP_STRING_LITERAL,
    JuxTokenTypes.INTERP_RAW_STRING_LITERAL,
    JuxTokenTypes.CHAR_LITERAL,
)
