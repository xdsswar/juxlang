package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * [JuxUnreachableCodeInspection] + [JuxRedundantSemicolonInspection]: the
 * structural syntax hints (with quick-fixes) the plugin surfaces without the
 * compiler.
 */
class JuxSyntaxHintInspectionsTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxUnreachableCodeInspection(),
            JuxRedundantSemicolonInspection(),
        )
    }

    private fun descriptions(code: String): List<String> {
        myFixture.configureByText("a.jux", code)
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    fun testUnreachableAfterReturnFlagged() {
        val d = descriptions(
            """
            package demo;
            public class A {
                public int go() {
                    return 1;
                    print("dead");
                }
            }
            """.trimIndent(),
        )
        assertTrue("dead code after return: $d", d.any { it == "Unreachable code" })
    }

    fun testReturnInsideIfIsNotUnreachable() {
        val d = descriptions(
            """
            package demo;
            public class A {
                public int go(int x) {
                    if (x > 0) { return 1; }
                    return 2;
                }
            }
            """.trimIndent(),
        )
        // The `return` is nested in the `if`, so `return 2;` is reachable.
        assertFalse("must not over-report: $d", d.any { it == "Unreachable code" })
    }

    fun testRemoveUnreachableQuickFix() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {
                public int go() {
                    return 1;
                    print("de<caret>ad");
                }
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        myFixture.launchAction(myFixture.findSingleIntention("Remove unreachable code"))
        assertFalse("dead tail removed", myFixture.file.text.contains("dead"))
        assertTrue("terminal kept", myFixture.file.text.contains("return 1;"))
    }

    fun testInspectionSurvivesMalformedTailAfterTerminal() {
        // Regression: error recovery after a terminal statement could yield a
        // zero-width statement, which createProblemDescriptor rejects (IDEA
        // throws). The inspections must skip empty-range anchors, so
        // highlighting completes on these half-typed buffers without throwing.
        val snippets = listOf(
            "package d; public class A { public int f() { return 1; . } }",
            "package d; public class A { public void f() { return; * } }",
            "package d; public class A { public void f() { return; @ } }",
            "package d; public class A { public void f() { return; ) } }",
        )
        for (s in snippets) {
            myFixture.configureByText("a.jux", s)
            myFixture.doHighlighting() // must not throw
        }
    }

    fun testRedundantSemicolonFlaggedAndFixed() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {
                public void go() {
                    var x = 1;
                    <caret>;
                }
            }
            """.trimIndent(),
        )
        val d = myFixture.doHighlighting().mapNotNull { it.description }
        assertTrue("empty statement flagged: $d", d.any { it == "Redundant semicolon" })
        myFixture.launchAction(myFixture.findSingleIntention("Remove redundant semicolon"))
        assertTrue("declaration kept", myFixture.file.text.contains("var x = 1;"))
    }
}
