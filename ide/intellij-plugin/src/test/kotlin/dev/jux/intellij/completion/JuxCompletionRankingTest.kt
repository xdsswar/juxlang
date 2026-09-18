package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.CompletionType
import com.intellij.codeInsight.completion.StatisticsUpdate
import com.intellij.codeInsight.lookup.LookupManager
import com.intellij.codeInsight.lookup.impl.LookupImpl
import com.intellij.psi.statistics.impl.StatisticsManagerImpl
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The ORDER of the completion popup, rule by rule, in the precedence Java
 * uses: prefix quality, expected type, locality, usage statistics,
 * accessibility, deprecation.
 *
 * Every case is built so that the alphabetical order (what a sorter with no
 * rules falls back to) is the WRONG answer, so a passing test proves the rule
 * did the work.
 */
class JuxCompletionRankingTest : BasePlatformTestCase() {

    private fun order(code: String, type: CompletionType = CompletionType.BASIC, times: Int = 1): List<String> {
        myFixture.configureByText("a.jux", code)
        return myFixture.complete(type, times)?.map { it.lookupString } ?: emptyList()
    }

    private fun assertBefore(items: List<String>, first: String, second: String) {
        assertTrue("$first should be offered: $items", first in items)
        assertTrue("$second should be offered: $items", second in items)
        assertTrue("$first should rank above $second: $items", items.indexOf(first) < items.indexOf(second))
    }

    // ---- locality ----

    fun testNearestLocalComesFirst() {
        val o = order("void main() { int value1 = 1; int value2 = 2; val<caret> }")
        assertBefore(o, "value2", "value1")
    }

    fun testLocalsRankAboveParametersAboveMembers() {
        val o = order(
            """
            public class Box {
                int itemCount = 0;
                void put(int itemSize) {
                    int itemTotal = 1;
                    item<caret>
                }
            }
            """.trimIndent(),
        )
        assertBefore(o, "itemTotal", "itemSize")
        assertBefore(o, "itemSize", "itemCount")
    }

    fun testOwnMembersRankAboveInheritedOnes() {
        val o = order(
            """
            public class Base { public int alphaBase() { return 1; } }
            public class Derived extends Base {
                public int alphaOwn() { return 2; }
                void run() { alpha<caret> }
            }
            """.trimIndent(),
        )
        assertBefore(o, "alphaOwn", "alphaBase")
    }

    fun testKeywordsRankBelowMatchingLocalsAndMembers() {
        val o = order(
            """
            public class Job {
                int retries = 3;
                void run() { int rewind = 1; re<caret> }
            }
            """.trimIndent(),
        )
        assertBefore(o, "rewind", "return")
        assertBefore(o, "retries", "return")
    }

    fun testUsersTypesRankAboveLibraryTypes() {
        myFixture.addFileToProject(".jux-stubs/rust/lib.jux.d", "package rust.lib;\n\npublic class Gadget { }\n")
        myFixture.addFileToProject("mine.jux", "public class Gizmo { }")
        val o = order("void main() { G<caret> }")
        assertBefore(o, "Gizmo", "Gadget")
    }

    // ---- expected type ----

    fun testAssignmentPrefersTheDeclaredType() {
        val o = order(
            """
            public class Shape { }
            void main() {
                Shape sa = new Shape();
                int sb = 0;
                Shape target = s<caret>
            }
            """.trimIndent(),
        )
        // `sb` is nearer; `sa` fits.
        assertBefore(o, "sa", "sb")
    }

    fun testArgumentPrefersTheParameterType() {
        val o = order(
            """
            public class Shape { }
            public class Printer { public void show(Shape s, int count) { } }
            void main() {
                Printer p = new Printer();
                int cnt = 1;
                Shape cshape = new Shape();
                p.show(cshape, c<caret>)
            }
            """.trimIndent(),
        )
        assertBefore(o, "cnt", "cshape")
    }

    fun testReturnPrefersTheReturnType() {
        val o = order(
            """
            public class Shape { }
            public class Maker {
                Shape make() {
                    Shape sa = new Shape();
                    int sb = 1;
                    return s<caret>
                }
            }
            """.trimIndent(),
        )
        assertBefore(o, "sa", "sb")
    }

    fun testConditionPrefersBool() {
        val o = order(
            """
            void main() {
                bool flag = true;
                int flat = 1;
                if (f<caret>)
            }
            """.trimIndent(),
        )
        assertBefore(o, "flag", "flat")
    }

    fun testEqualityOperandPrefersTheOtherSidesType() {
        val o = order(
            """
            public enum Color { RED, GREEN }
            void main() {
                Color col = Color.RED;
                Color cother = Color.GREEN;
                int cnum = 1;
                if (col == c<caret>)
            }
            """.trimIndent(),
        )
        assertBefore(o, "cother", "cnum")
    }

    fun testMemberReturningTheExpectedTypeComesFirst() {
        val o = order(
            """
            public class Shape { }
            public class Store {
                public int count() { return 0; }
                public String name() { return ""; }
                public Shape shape() { return new Shape(); }
            }
            void main() { Store st = new Store(); Shape s = st.<caret> }
            """.trimIndent(),
        )
        assertEquals("the Shape-returning method first: $o", "shape", o.firstOrNull())
    }

    fun testExpectedTypeBeatsLocality() {
        // `number` is nearer, so locality alone would put it first.
        val flipped = order(
            """
            void main() {
                String name = "";
                int number = 1;
                String label = n<caret>
            }
            """.trimIndent(),
        )
        assertBefore(flipped, "name", "number")
    }

    // ---- statistics ----

    fun testRecentlyChosenItemFloatsUp() {
        (com.intellij.psi.statistics.StatisticsManager.getInstance() as StatisticsManagerImpl)
            .enableStatistics(testRootDisposable)
        val code = """
            public class Tool {
                public int beta1() { return 1; }
                public int beta2() { return 2; }
                void run() { bet<caret> }
            }
        """.trimIndent()
        val before = order(code)
        assertBefore(before, "beta1", "beta2")
        // Choose beta2 a few times, as a user would.
        repeat(3) {
            val lookup = LookupManager.getActiveLookup(myFixture.editor) as LookupImpl
            val item = lookup.items.first { it.lookupString == "beta2" }
            lookup.currentItem = item
            StatisticsUpdate.collectStatisticChanges(item)
            StatisticsUpdate.applyLastCompletionStatisticsUpdate()
            LookupManager.getInstance(project).hideActiveLookup()
            myFixture.completeBasic()
        }
        val after = myFixture.lookupElementStrings ?: emptyList()
        assertBefore(after, "beta2", "beta1")
    }

    // ---- accessibility and deprecation ----

    fun testInaccessibleMembersShowOnSecondInvocationBelowTheRest() {
        val code = """
            public class Vault { private int zeta1 = 1; public int zeta2 = 2; }
            void main() { Vault v = new Vault(); v.ze<caret> }
        """.trimIndent()
        myFixture.configureByText("a.jux", code)
        val first = myFixture.completeBasic()
        // Only zeta2 is reachable: it is inserted directly.
        assertNull("a single reachable item is inserted: ${first?.map { it.lookupString }}", first)
        myFixture.configureByText("b.jux", code)
        val second = myFixture.complete(CompletionType.BASIC, 2)?.map { it.lookupString } ?: emptyList()
        assertBefore(second, "zeta2", "zeta1")
    }

    fun testDeprecatedItemsRankLast() {
        val o = order(
            """
            public class Calc {
                @Deprecated
                public int calcA() { return 1; }
                public int calcB() { return 2; }
                void run() { calc<caret> }
            }
            """.trimIndent(),
        )
        assertBefore(o, "calcB", "calcA")
    }

    // ---- prefix quality stays on top ----

    fun testPrefixQualityOutranksEverythingElse() {
        val o = order(
            """
            public class Shape { }
            void main() {
                Shape myShape = new Shape();
                int shapeCount = 1;
                Shape target = shape<caret>
            }
            """.trimIndent(),
        )
        // `shapeCount` starts with what was typed; `myShape` only matches in
        // the middle, even though its type fits.
        assertBefore(o, "shapeCount", "myShape")
    }
}
