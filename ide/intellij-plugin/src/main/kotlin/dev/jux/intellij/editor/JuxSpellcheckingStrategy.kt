package dev.jux.intellij.editor

import com.intellij.psi.PsiElement
import com.intellij.spellchecker.tokenizer.SpellcheckingStrategy
import com.intellij.spellchecker.tokenizer.TokenConsumer
import com.intellij.spellchecker.tokenizer.Tokenizer
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxNamedElement

/**
 * Spellchecking for Jux: prose in comments and plain strings, camelCase-split
 * identifiers in declarations, and silence everywhere else.
 *
 * Two deliberate exclusions, both because a false squiggle in code is worse
 * than a missed typo in prose:
 *
 * - **Interpolated strings** are skipped whole. `$"total: ${qty * unitPrice}"`
 *   is half prose and half code, and the tokenizer has no way to tell the two
 *   apart from the token text alone. Checking it would underline every
 *   identifier that appears in a hole.
 * - **References** are not checked, only declarations. A misspelled name is
 *   worth flagging once, where it is introduced; flagging it again at every use
 *   site turns one typo into twenty warnings, and a name from a foreign crate
 *   is not the author's to fix at all.
 */
class JuxSpellcheckingStrategy : SpellcheckingStrategy() {

    override fun getTokenizer(element: PsiElement): Tokenizer<*> {
        val type = element.node?.elementType ?: return EMPTY_TOKENIZER

        // Comments and doc comments: ordinary prose.
        if (JuxTokenTypes.COMMENTS.contains(type)) return TEXT_TOKENIZER

        // A plain (non-interpolated) string or char literal is prose too. The
        // quotes are part of the token, so the platform's own literal
        // tokenizer would need escape handling we do not have; treating the
        // token as text is close enough and never wrong about code.
        if (type === JuxTokenTypes.STRING_LITERAL || type === JuxTokenTypes.RAW_STRING_LITERAL) {
            return TEXT_TOKENIZER
        }

        // A declaration's own name, split on camelCase / underscores.
        if (element is JuxNamedElement) return super.getTokenizer(element)

        return EMPTY_TOKENIZER
    }

    private companion object {
        /**
         * A tokenizer that reports nothing. Returned for every element the
         * strategy has no opinion about, so the platform never falls back to
         * checking raw source text.
         */
        val EMPTY_TOKENIZER: Tokenizer<PsiElement> = object : Tokenizer<PsiElement>() {
            override fun tokenize(element: PsiElement, consumer: TokenConsumer) = Unit
        }
    }
}
