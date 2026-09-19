package dev.jux.intellij.documentation

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Quick Documentation (Ctrl+Q) renders a doc comment the way Java's popup
 * does: signature with its owner, the description, and Params / Returns /
 * Throws sections, for project sources and `.jux.d` stubs alike.
 */
class JuxQuickDocTest : BasePlatformTestCase() {

    /** The rendered doc of what the reference at `<caret>` resolves to. */
    private fun docAtCaret(): String {
        val target = myFixture.file.findReferenceAt(myFixture.caretOffset)?.resolve()
        assertNotNull("the caret's reference resolves", target)
        return JuxDocumentationProvider().generateDoc(target, null) ?: ""
    }

    fun testJavadocTagsBecomeSections() {
        myFixture.configureByText(
            "a.jux",
            """
            package shop;
            class Cart {
                /**
                 * Adds {@code qty} items, see `total`.
                 *
                 * A second paragraph.
                 * @param qty how many to add,
                 *     never negative
                 * @return the new total
                 * @throws IllegalArgumentException when qty < 0
                 */
                public int add(int qty) throws IllegalArgumentException { return qty; }
            }
            void main() { new Cart().a<caret>dd(1); }
            """.trimIndent(),
        )
        val doc = docAtCaret()
        assertTrue(doc, doc.contains("shop.Cart"))
        assertTrue(doc, doc.contains("public int add(int qty) throws IllegalArgumentException"))
        assertTrue(doc, doc.contains("Adds <code>qty</code> items, see <code>total</code>."))
        assertTrue(doc, doc.contains("</p><p>A second paragraph."))
        assertTrue(doc, doc.contains("Params:"))
        assertTrue(doc, doc.contains("<code>qty</code> - <p>how many to add, never negative</p>"))
        assertTrue(doc, doc.contains("Returns:"))
        assertTrue(doc, doc.contains("the new total"))
        assertTrue(doc, doc.contains("<code>IllegalArgumentException</code> - <p>when qty &lt; 0</p>"))
        assertFalse("tags do not leak into the description: $doc", doc.contains("@param"))
    }

    fun testOverrideWithoutDocShowsTheOverriddenOne() {
        myFixture.configureByText(
            "a.jux",
            """
            interface Shape {
                /** The area in square units. */
                double area();
            }
            class Sq implements Shape { public double area() { return 1.0; } }
            void main() { new Sq().ar<caret>ea(); }
            """.trimIndent(),
        )
        val doc = docAtCaret()
        assertTrue(doc, doc.contains("Description copied from: Shape.area"))
        assertTrue(doc, doc.contains("The area in square units."))
    }

    fun testVarLocalShowsItsInferredType() {
        myFixture.configureByText(
            "a.jux",
            "class Truck {}\nvoid main() { var t = new Truck(); print(<caret>t); }",
        )
        val doc = docAtCaret()
        assertTrue(doc, doc.contains("var t = new Truck(): Truck"))
    }

    fun testStubMemberDocRenders() {
        myFixture.addFileToProject(
            "Stack.jux.d",
            """
            public class Stack {
                /// Pushes a value on top.
                /// @param value what to push
                public void push(int value);
            }
            """.trimIndent(),
        )
        myFixture.configureByText("a.jux", "void main() { var s = new Stack(); s.pu<caret>sh(1); }")
        val doc = docAtCaret()
        assertTrue(doc, doc.contains("public void push(int value)"))
        assertTrue(doc, doc.contains("Pushes a value on top."))
        assertTrue(doc, doc.contains("<code>value</code> - <p>what to push</p>"))
    }

    fun testParseSplitsTags() {
        val d = JuxDocComment.parse("Main text.\n@param <T> the element\n@since 0.1\n@see Other")
        assertEquals("Main text.", d.description)
        assertEquals(listOf("&lt;T&gt;" to "the element"), d.params)
        assertEquals("0.1", d.since)
        assertEquals(listOf("Other"), d.see)
    }
}
