package dev.jux.intellij.templates

import com.intellij.codeInsight.template.impl.TemplateManagerImpl
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

    // ---- expansion, filtering and positions --------------------------------

    /** Put [before] (with `<caret>` after the key) in a file, press Tab, return the text. */
    private fun tab(before: String): String {
        TemplateManagerImpl.setTemplateTesting(testRootDisposable)
        myFixture.configureByText("a.jux", before.trimIndent())
        myFixture.type('\t')
        return myFixture.editor.document.text
    }

    private fun inMain(body: String) = "public void main() {\n$body\n}\n"

    fun testIfWrapsABooleanStatement() {
        val out = tab(inMain("    bool ready = true;\n    ready.if<caret>"))
        assertTrue(out, out.contains("if (ready) {"))
    }

    fun testIfIsNotOfferedOnAString() {
        val out = tab(inMain("    \"text\".if<caret>"))
        assertFalse(out, out.contains("if (\"text\")"))
    }

    fun testStatementTemplatesNeedAStatementPosition() {
        val out = tab(inMain("    bool ready = true;\n    bool x = ready.if<caret>"))
        assertFalse(out, out.contains("if (ready)"))
    }

    fun testVarNamesTheValueAfterItsType() {
        val out = tab(inMain("    new Point(1, 2).var<caret>"))
        assertTrue(out, out.contains("var point = new Point(1, 2);"))
    }

    fun testForNamesTheItemAfterTheCollection() {
        val out = tab(inMain("    String[] names = new String[] { \"a\" };\n    names.for<caret>"))
        assertTrue(out, out.contains("for (var name : names) {"))
    }

    fun testForiRunsToTheArrayLength() {
        val out = tab(inMain("    int[] xs = new int[] { 1, 2 };\n    xs.fori<caret>"))
        assertTrue(out, out.contains("for (int i = 0; i < xs.length; i++) {"))
    }

    fun testForrCountsDown() {
        val out = tab(inMain("    int n = 3;\n    n.forr<caret>"))
        assertTrue(out, out.contains("for (int i = n - 1; i >= 0; i--) {"))
    }

    fun testNotParenthesizesACompoundExpression() {
        val out = tab(inMain("    bool a = true;\n    bool b = (a == false).not<caret>;"))
        assertTrue(out, out.contains("bool b = !(a == false);"))
    }

    fun testNullCheckIsNotOfferedOnAConstructedValue() {
        val out = tab(inMain("    new Point(1, 2).null<caret>"))
        assertFalse(out, out.contains("== null"))
    }

    fun testNullCheckOnANullableValue() {
        val out = tab(inMain("    String? name = null;\n    name.nn<caret>"))
        assertTrue(out, out.contains("if (name != null) {"))
    }

    fun testNewConstructsAType() {
        val out = tab(inMain("    var p = Point.new<caret>;"))
        assertTrue(out, out.contains("var p = new Point();"))
    }

    fun testSoutPrints() {
        val out = tab(inMain("    int n = 3;\n    n.sout<caret>"))
        assertTrue(out, out.contains("print(n);"))
    }

    fun testTryWrapsACall() {
        val out = tab(inMain("    load().try<caret>"))
        assertTrue(out, out.contains("try {"))
        assertTrue(out, out.contains("load();"))
        assertTrue(out, out.contains("} catch (Exception e) {"))
    }

    fun testCastWrapsTheExpression() {
        val out = tab(inMain("    var x = value.cast<caret>;"))
        assertTrue(out, out.contains("var x = ((Type) value);"))
    }

    fun testJavaTemplateSetIsRegistered() {
        val names = JuxPostfixTemplateProvider().templates.map { it.key }
        assertContainsElements(
            names,
            ".if", ".else", ".while", ".not", ".assert", ".null", ".notnull", ".nn",
            ".for", ".fori", ".forr", ".var", ".return", ".throw", ".print", ".sout",
            ".switch", ".try", ".par", ".cast", ".arg", ".await", ".new",
        )
    }
}
