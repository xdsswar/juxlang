package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * E0464 in the editor (JUX-MISSING-DEFS §M.14.2, ERRATA E95), case for case
 * with the compiler's `final_*_reassign` tests in `juxc-tycheck`: a `final` or
 * `const` parameter, local or for-each binder may be read and never stored
 * into again, a plain or shadowing binding may, and `final ref` stores
 * through its cell.
 */
class JuxFinalReassignmentInspectionTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(JuxFinalReassignmentInspection())
    }

    private fun descriptions(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting().mapNotNull { it.description }.filter { "E0464" in it }
    }

    fun testFinalParameterReassignIsE0464() {
        val d = descriptions("public void f(final int x) { x = 5; }")
        assertEquals(d.toString(), 1, d.size)
        assertTrue(d.toString(), "cannot reassign `x`: it is a `final` binding" in d[0])
    }

    fun testCompoundAssignAndIncrementAreE0464() {
        val d = descriptions(
            """
            public void f(final int x) {
                x += 1;
                x++;
                --x;
            }
            """,
        )
        assertEquals(d.toString(), 3, d.size)
    }

    fun testFinalLocalReassignIsE0464() {
        val d = descriptions("public void f() { final int y = 1; y = 2; }")
        assertEquals(d.toString(), 1, d.size)
    }

    fun testFinalForEachBinderReassignIsE0464() {
        assertEquals(1, descriptions("public void f() { for (final int n : 0..3) { n = 9; } }").size)
        assertEquals(1, descriptions("public void f() { for (final int n : 0..3) { n += 1; } }").size)
    }

    fun testMessageEchoesTheWrittenKeyword() {
        val d = descriptions("public void f() { for (const int n : 0..3) { n = 9; } }")
        assertEquals(d.toString(), 1, d.size)
        assertTrue(d.toString(), "it is a `const` binding" in d[0] && "Drop `const`" in d[0])
        assertFalse(d.toString(), "`final`" in d[0])
    }

    fun testReadingAFinalBindingIsFine() {
        assertEmpty(descriptions("public void f(final int x) { var y = x + 1; }"))
        assertEmpty(descriptions("public void f() { var t = 0; for (final int n : 0..3) { t += n; } }"))
    }

    fun testPlainBindingsMayBeReassigned() {
        assertEmpty(descriptions("public void f(int x) { x = 5; }"))
        assertEmpty(descriptions("public void f() { for (int n : 0..3) { n = 9; } }"))
    }

    fun testAPlainForEachBinderShadowsAnOuterFinal() {
        assertEmpty(descriptions("public void f() { final int n = 1; for (int n : 0..3) { n = 9; } }"))
    }

    fun testFinalRefStoresThroughItsCell() {
        assertEmpty(descriptions("public void f(final ref int x) { x = 5; }"))
    }

    fun testDropFinalFix() {
        myFixture.configureByText("a.jux", "public void f() { final int y = 1; y = 2; }")
        myFixture.doHighlighting()
        val fix = myFixture.getAllQuickFixes().firstOrNull { it.text == "Drop 'final'" }
            ?: error("no fix among ${myFixture.getAllQuickFixes().map { it.text }}")
        myFixture.launchAction(fix)
        assertFalse(myFixture.editor.document.text, "final" in myFixture.editor.document.text)
        assertTrue(myFixture.editor.document.text, "int y = 1;" in myFixture.editor.document.text)
    }
}
