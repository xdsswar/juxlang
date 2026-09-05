package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Member completion after a `.` for receivers that are not a bare name.
 *
 * The fallback resolved a receiver by scanning identifier characters backwards
 * from the dot, so anything ending in `)`, `]`, `!` or `?` produced no receiver
 * at all — and because the after-dot branch returns unconditionally, the popup
 * was empty rather than falling back to something. These are the shapes real
 * code is full of.
 *
 * (`JuxLspState.isServing` is hardwired `false` under the fixture, so every
 * test here exercises the fallback by construction.)
 */
class JuxReceiverShapesTest : BasePlatformTestCase() {

    /** A small world with a chain worth walking. */
    private fun world(main: String): List<String> {
        myFixture.configureByText(
            "a.jux",
            """
            public class Leaf {
                public int size() { return 1; }
                public String label = "";
            }
            public class Node {
                public Leaf leaf;
                public Leaf make() { return new Leaf(); }
                public Leaf? maybe() { return null; }
            }
            public enum Color { Red, Green }
            void main() {
                Node n = new Node();
                Vec<Leaf> xs = new Vec<Leaf>();
                $main
            }
            """.trimIndent(),
        )
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    fun testFieldChainReceiver() {
        val o = world("n.leaf.<caret>")
        assertTrue("a field's type resolves: $o", o.containsAll(listOf("size", "label")))
    }

    fun testCallReturnReceiver() {
        val o = world("n.make().<caret>")
        assertTrue("a call's return type resolves: $o", o.containsAll(listOf("size", "label")))
    }

    fun testNotNullAssertedReceiver() {
        val o = world("n.maybe()!!.<caret>")
        assertTrue("`!!` unwraps to the inner type: $o", o.containsAll(listOf("size", "label")))
    }

    fun testSafeCallReceiver() {
        val o = world("n.maybe()?.<caret>")
        assertTrue("`?.` reads through the nullable: $o", o.containsAll(listOf("size", "label")))
    }

    fun testIndexedReceiver() {
        val o = world("xs[0].<caret>")
        assertTrue("an index read yields the element type: $o", o.containsAll(listOf("size", "label")))
    }

    fun testParenthesizedReceiver() {
        val o = world("(n.leaf).<caret>")
        assertTrue("parentheses are transparent: $o", o.containsAll(listOf("size", "label")))
    }

    fun testEnumConstantReceiver() {
        val o = world("Color.Red.<caret>")
        // An enum constant is a value of its enum, so instance members apply —
        // there are none here, but it must not offer the STATIC surface again.
        assertFalse("a constant is not the enum type itself: $o", o.contains("Red"))
    }

    /** An unresolvable receiver must still produce nothing, not garbage. */
    fun testUnknownReceiverStaysEmpty() {
        val o = world("whoKnows().<caret>")
        assertTrue("nothing invented for an unknown receiver: $o", o.isEmpty())
    }
}
