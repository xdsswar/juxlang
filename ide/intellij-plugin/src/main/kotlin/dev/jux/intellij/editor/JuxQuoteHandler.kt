package dev.jux.intellij.editor

import com.intellij.codeInsight.editorActions.SimpleTokenSetQuoteHandler
import dev.jux.intellij.highlight.JuxTokenTypes

/**
 * Auto-closes `"` and `'`: typing the opening quote inserts the matching close
 * and parks the caret between them, and typing the closing quote over an
 * existing one steps past it (no duplicate). Driven by the lexer's STRING and
 * CHAR token types.
 */
class JuxQuoteHandler : SimpleTokenSetQuoteHandler(JuxTokenTypes.STRING, JuxTokenTypes.CHAR)
