package dev.jux.intellij.editor

import com.intellij.lang.BracePair
import com.intellij.lang.PairedBraceMatcher
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IElementType
import dev.jux.intellij.highlight.JuxTokenTypes

/**
 * Pairs `{}`, `[]`, and `()` so the editor auto-inserts the matching close
 * (caret in the middle), highlights matching braces, and — for `{` — enables
 * the platform's "Enter between braces" behaviour (press Enter after `{` to
 * split onto an indented line with `}` below).
 */
class JuxBraceMatcher : PairedBraceMatcher {
    override fun getPairs(): Array<BracePair> = PAIRS

    override fun isPairedBracesAllowedBeforeType(lbraceType: IElementType, contextType: IElementType?): Boolean = true

    override fun getCodeConstructStart(file: PsiFile?, openingBraceOffset: Int): Int = openingBraceOffset

    private companion object {
        val PAIRS = arrayOf(
            BracePair(JuxTokenTypes.LBRACE, JuxTokenTypes.RBRACE, true),
            BracePair(JuxTokenTypes.LBRACKET, JuxTokenTypes.RBRACKET, false),
            BracePair(JuxTokenTypes.LPAREN, JuxTokenTypes.RPAREN, false),
        )
    }
}
