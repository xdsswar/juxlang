package dev.jux.intellij.templates

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Postfix templates — `expr.if`, `expr.var`, and the rest.
 *
 * They are worth their own tests for two reasons. They are the completion
 * people reach for most while typing, and they are on a different extension
 * point from the completion contributor, so they are the one kind of depth the
 * plugin keeps while `juxc-lsp` is serving everything else.
 */
class JuxPostfixTemplateTest : BasePlatformTestCase() {

    private fun expand(before: String, key: String): String {
        myFixture.configureByText("a.jux", before)
        myFixture.type(key)
        myFixture.completeBasic()
        return myFixture.editor.document.text
    }

    // ---- the expression scan, which is what makes these work on real code ----

    fun testExpressionScanTakesAWholeChain() {
        // `f(a, b).c[0]` is one expression: the scan tracks bracket depth
        // rather than stopping at the last word.
        val chars = "var x = f(a, b).c[0]."
        val start = JuxPostfixTemplate.expressionStartForTest(chars, chars.length - 1)
        assertEquals("f(a, b).c[0]", chars.substring(start!!, chars.length - 1))
    }

    fun testExpressionScanStopsAtAnAssignment() {
        val chars = "var x = y."
        val start = JuxPostfixTemplate.expressionStartForTest(chars, chars.length - 1)
        assertEquals("y", chars.substring(start!!, chars.length - 1))
    }

    fun testExpressionScanRejectsABareKeyword() {
        val chars = "    return."
        assertNull(JuxPostfixTemplate.expressionStartForTest(chars, chars.length - 1))
    }

    fun testExpressionScanRejectsNothingBeforeTheDot() {
        val chars = "    ."
        assertNull(JuxPostfixTemplate.expressionStartForTest(chars, chars.length - 1))
    }

    // ---- the templates are registered and offered ---------------------------

    fun testTemplatesAreRegistered() {
        val names = JuxPostfixTemplateProvider().templates.map { it.key }
        assertContainsElements(names, ".if", ".var", ".for", ".not", ".return", ".print")
    }
}
