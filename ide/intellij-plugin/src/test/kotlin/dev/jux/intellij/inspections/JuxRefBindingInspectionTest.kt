package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The `ref` binding rules (JUX-MISSING-DEFS §M.13, ERRATA E84): the deferred
 * `ref` return type (E0523), nesting (E0524), a `ref` generic argument
 * (E0526), the pointless `ref` on an already-shared type (W0490), and a `ref`
 * captured by a `Worker.spawn` closure (E0702).
 *
 * The W0490 negatives carry the weight here. `ref` on a value type is the
 * ordinary, correct use of the feature, so a warning on `ref String`,
 * `ref int`, a record, a struct or an enum would be worse than no warning at
 * all: it would tell the user to delete the one thing making their code work.
 */
class JuxRefBindingInspectionTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(JuxRefBindingInspection())
    }

    private fun descriptions(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    // ---- E0523: a `ref` return type -----------------------------------------

    fun testRefReturnTypeOnAFreeFunctionIsRejected() {
        val d = descriptions(
            """
            ref int counter() { return 1; }
            """,
        )
        assertTrue("E0523 for a free function: $d", d.any { it.contains("E0523") })
    }

    fun testRefReturnTypeOnAMethodIsRejected() {
        val d = descriptions(
            """
            public class Box {
                public ref String label() { return "x"; }
            }
            """,
        )
        assertTrue("E0523 for a method: $d", d.any { it.contains("E0523") })
    }

    fun testRefFieldIsNotAReturnType() {
        val d = descriptions(
            """
            public class Profile {
                public ref String displayName = "a";
                public static ref int counter = 0;
            }
            """,
        )
        // §M.13.4 allows `ref` on a field, `static` included. Neither line is
        // a return type, and `String`/`int` are value types, so nothing fires.
        assertTrue("a ref field is legal: $d", d.none { it.contains("E0523") || it.contains("W0490") })
    }

    // ---- E0524: `ref ref T` -------------------------------------------------

    fun testNestedRefIsRejected() {
        val d = descriptions(
            """
            void main() {
                ref ref int x = 1;
            }
            """,
        )
        assertTrue("E0524: $d", d.any { it.contains("E0524") })
    }

    fun testSingleRefDoesNotReportNesting() {
        val d = descriptions(
            """
            void main() {
                ref int x = 1;
            }
            """,
        )
        assertTrue("no E0524 for one ref: $d", d.none { it.contains("E0524") })
    }

    // ---- E0526: a `ref` generic argument ------------------------------------

    fun testRefGenericArgumentIsRejected() {
        val d = descriptions(
            """
            public class Holder {
                public Vec<ref int> items;
            }
            """,
        )
        assertTrue("E0526: $d", d.any { it.contains("E0526") })
    }

    // ---- W0490: `ref` on a type that is already a reference -----------------

    fun testRefOnAClassWarns() {
        val d = descriptions(
            """
            public class Counter {
                public int n = 0;
            }
            void main() {
                ref Counter c = new Counter();
            }
            """,
        )
        assertTrue("W0490 naming the class: $d", d.any { it.contains("W0490") && it.contains("`Counter`") })
    }

    fun testRefOnAnInterfaceWarns() {
        val d = descriptions(
            """
            public interface Shape {
                double area();
            }
            public class Holder {
                public ref Shape s;
            }
            """,
        )
        assertTrue("W0490 naming the interface: $d", d.any { it.contains("W0490") && it.contains("interface") })
    }

    fun testRefOnAnArrayWarns() {
        val d = descriptions(
            """
            void main() {
                ref int[] values = new int[3];
            }
            """,
        )
        assertTrue("W0490 for an array: $d", d.any { it.contains("W0490") && it.contains("array") })
    }

    fun testRefOnValueTypesIsSilent() {
        val d = descriptions(
            """
            public record Point(int x, int y) {}
            public struct Pair {
                public int a = 0;
            }
            public enum Color { RED, GREEN }
            void main(ref String name, ref int count) {
                ref Point p = new Point(1, 2);
                ref Pair q = new Pair();
                ref Color c = Color.RED;
                ref double d = 1.0;
                ref string s = "x";
            }
            """,
        )
        assertTrue("`ref` on a value type never warns: $d", d.none { it.contains("W0490") })
    }

    fun testRemoveRefQuickFix() {
        myFixture.configureByText(
            "a.jux",
            """
            public class Counter {
                public int n = 0;
            }
            void main() {
                <caret>ref Counter c = new Counter();
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val fix = myFixture.findSingleIntention("Remove the redundant `ref`")
        myFixture.launchAction(fix)
        assertTrue("the keyword is gone: ${myFixture.file.text}", myFixture.file.text.contains("Counter c = new Counter();"))
        assertFalse("no `ref` left: ${myFixture.file.text}", myFixture.file.text.contains("ref Counter"))
    }

    // ---- E0702: a `ref` binding captured by a spawned closure ---------------

    fun testRefCapturedBySpawnIsRejected() {
        val d = descriptions(
            """
            void main() {
                ref int total = 0;
                Worker.spawn(() -> { print(total); });
            }
            """,
        )
        assertTrue("E0702 for a ref capture: $d", d.any { it.contains("E0702") && it.contains("total") })
    }

    fun testPlainLocalCapturedBySpawnIsSilent() {
        val d = descriptions(
            """
            void main() {
                int total = 0;
                Worker.spawn(() -> { print(total); });
            }
            """,
        )
        assertTrue("a plain local crosses fine: $d", d.none { it.contains("E0702") })
    }

    // ---- the TYPE of a `ref` binding is plain `T` ---------------------------

    fun testRefBindingKeepsThePlainTypeForCompletion() {
        myFixture.configureByText(
            "a.jux",
            """
            public class Counter {
                public int ticks = 0;
                public void bump() {}
            }
            void main() {
                ref Counter c = new Counter();
                c.<caret>
            }
            """.trimIndent(),
        )
        val names = myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
        // §M.13.2: the expression type of a `ref T` binding is `T`, so the
        // members offered are the ones a plain `Counter` would offer.
        assertTrue("members of Counter through a ref binding: $names", names.containsAll(listOf("ticks", "bump")))
    }
}
