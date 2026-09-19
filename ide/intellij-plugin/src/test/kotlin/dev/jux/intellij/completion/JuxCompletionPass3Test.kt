package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Completion added in 0.1.0: the missing arms right after `case`, and chains
 * that reach the wanted type (Ctrl+Shift+Space twice in Java).
 */
class JuxCompletionPass3Test : BasePlatformTestCase() {

    private fun offered(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        val items = myFixture.completeBasic() ?: return listOfNotNull(wordBeforeCaret())
        return items.map { it.lookupString }
    }

    private fun wordBeforeCaret(): String? {
        val text = myFixture.editor.document.charsSequence
        var start = myFixture.caretOffset
        while (start > 0 && text[start - 1] != ' ') start--
        return text.subSequence(start, myFixture.caretOffset).toString().ifEmpty { null }
    }

    // ---- after `case` -------------------------------------------------------

    fun testCaseOffersOnlyTheMissingEnumConstantsInOrder() {
        val o = offered(
            """
            enum Color { Red, Green, Blue }
            void f(Color c) {
                switch (c) {
                    case Green -> { }
                    case <caret>
                }
            }
            """,
        )
        assertEquals(listOf("Red", "Blue"), o.filter { it in setOf("Red", "Green", "Blue") })
    }

    fun testCaseOffersSealedSubtypesAsTypePatterns() {
        val o = offered(
            """
            sealed interface Shape permits Circle, Square {}
            record Circle(double r) implements Shape {}
            record Square(double side) implements Shape {}
            double f(Shape s) {
                return switch (s) {
                    case Circle c -> 1.0;
                    case <caret>
                };
            }
            """,
        )
        assertTrue(o.toString(), o.any { it == "Square square" || it == "Square" })
        assertFalse(o.toString(), o.any { it.startsWith("Circle") })
    }

    fun testPayloadVariantGetsItsBinders() {
        val o = offered(
            """
            enum Shape { Dot, Circle(double) }
            void f(Shape s) {
                switch (s) {
                    case Dot -> { }
                    case <caret>
                }
            }
            """,
        )
        assertTrue(o.toString(), o.contains("Circle(_)"))
    }
}
