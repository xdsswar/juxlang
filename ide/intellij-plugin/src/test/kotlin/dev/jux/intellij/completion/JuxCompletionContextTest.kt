package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Context-aware keyword completion: the set offered must match the grammar
 * position — statements in a block, members in a class body, declarations at
 * the top level. (plugin-gap.md D2.)
 *
 * Carets sit on **empty positions** (no prefix): a prefix that narrows to a
 * single lookup element makes the fixture auto-insert it and return an empty
 * lookup list, which would fail the assertions for the wrong reason.
 */
class JuxCompletionContextTest : BasePlatformTestCase() {

    private fun completionsAt(code: String): List<String> {
        myFixture.configureByText("a.jux", code)
        myFixture.completeBasic()
        return myFixture.lookupElementStrings ?: emptyList()
    }

    fun testStatementPositionOffersStatementsNotDeclarations() {
        val items = completionsAt(
            """
            package demo;
            public class A {
                public void go() {
                    <caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "return", "if", "while", "var")
        assertDoesntContain(items, "class", "package", "import", "implements")
    }

    fun testClassBodyOffersMembersNotStatements() {
        val items = completionsAt(
            """
            package demo;
            public class A {
                <caret>
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "public", "class")
        assertDoesntContain(items, "return", "break", "continue")
    }

    fun testTopLevelOffersDeclarations() {
        val items = completionsAt("<caret>")
        assertContainsElements(items, "package", "import", "public", "class")
        assertDoesntContain(items, "return", "this")
    }

    /** Newer-surface keywords land in the right contexts (language-sync wave). */
    fun testRecentKeywordsOfferedInTheirContexts() {
        // `yield` / `case` / `default` are statement-position words (switch
        // bodies); `sizeof` starts an expression.
        val stmt = completionsAt(
            """
            public class A {
                public void go() {
                    <caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(stmt, "yield", "case", "default", "sizeof")

        // `permits` belongs to type headers — member context (nested types)
        // and top level both offer it; statements must not.
        val member = completionsAt(
            """
            public class A {
                <caret>
            }
            """.trimIndent(),
        )
        assertContainsElements(member, "permits", "extends", "implements")
        assertDoesntContain(stmt, "permits")

        val top = completionsAt("<caret>")
        assertContainsElements(top, "extends", "implements", "permits")
    }
}
