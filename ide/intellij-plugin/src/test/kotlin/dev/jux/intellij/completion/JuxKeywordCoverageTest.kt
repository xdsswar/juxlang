package dev.jux.intellij.completion

import dev.jux.intellij.highlight.JuxKeywords
import junit.framework.TestCase

/**
 * Drift gate between the generated token alphabet and the completion
 * tables: every reserved keyword the lexer knows must be offerable in at
 * least one grammar position, and the literal constants must ride the
 * expression set. When the compiler reserves a NEW keyword (regenerating
 * `jux-tokens.json`), this test fails until someone decides which
 * [JuxKeywordContext] set(s) it belongs to — instead of the word silently
 * never appearing in completion.
 */
class JuxKeywordCoverageTest : TestCase() {

    fun testEveryKeywordIsOfferedSomewhere() {
        val offered = JuxKeywordContext.TOP_LEVEL +
            JuxKeywordContext.MEMBER +
            JuxKeywordContext.STATEMENT +
            JuxKeywordContext.EXPRESSION +
            JuxKeywordContext.ACCESSOR
        val missing = JuxKeywords.KEYWORDS - offered
        assertTrue(
            "keywords reserved by the lexer but absent from every completion context " +
                "(add them to the right JuxKeywordContext set): $missing",
            missing.isEmpty(),
        )
    }

    fun testLiteralConstantsCompleteInExpressions() {
        // true / false / null are CONSTANTS (literal tokens), not keywords —
        // they must still complete where an expression can begin.
        assertTrue(JuxKeywordContext.EXPRESSION.containsAll(JuxKeywords.CONSTANTS))
    }

    fun testCurationDroppedNothingIntended() {
        // Spot-pins for the pre-wired words that started life curated-out:
        // both are reserved now, so both must be present.
        assertTrue("typeof reserved but not offered", "typeof" in JuxKeywordContext.EXPRESSION)
        assertTrue("ref reserved but not offered", "ref" in JuxKeywordContext.STATEMENT)
    }
}
