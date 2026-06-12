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

    // ---- observable properties (§P) ------------------------------------------

    fun testAccessorBlockOffersGetSetAndVisibilityOnly() {
        val items = completionsAt(
            """
            public class A {
                public String Name { <caret> }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "get", "set", "private", "protected")
        assertDoesntContain(items, "class", "return", "if", "observer")
    }

    fun testObserverOfferedInMemberAndStatementPositions() {
        val member = completionsAt(
            """
            public class A {
                <caret>
            }
            """.trimIndent(),
        )
        assertContainsElements(member, "observer")

        val stmt = completionsAt(
            """
            public class A {
                public void go() {
                    <caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(stmt, "observer")
    }

    fun testAfterObserversDotOffersTheFourOps() {
        val items = completionsAt(
            """
            public class A {
                public String Name { get; set; } = "";
                public void go() {
                    Name.observers.<caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "attach", "detach", "clear", "size")
        assertDoesntContain(items, "if", "return", "Name")
    }

    fun testAfterPropertyDotOffersObserversAndBindOps() {
        val items = completionsAt(
            """
            public class A {
                public String Name { get; set; } = "";
                public void go() {
                    Name.<caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "observers", "bind", "unbind", "bindBidirectional")
        assertDoesntContain(items, "if", "return")
    }

    fun testAfterNonPropertyDotOffersNothing() {
        val items = completionsAt(
            """
            public class A {
                private int plain;
                public void go(A other) {
                    other.<caret>
                }
            }
            """.trimIndent(),
        )
        // Member completion is juxc-lsp territory — the fallback stays empty.
        assertEmpty(items)
    }

    // ---- relevance ordering ----------------------------------------------------

    /**
     * The relevance contract: visible locals/params float above enclosing-class
     * members, which float above keywords and file-level types. Locals of other
     * methods and members of other classes never appear at all.
     */
    fun testRelevanceOrderingAndScopeFiltering() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Helper {
                public int helperField;
                public void helperMethod() {
                    var helperLocal = 1;
                }
            }
            public class A {
                private String myField;
                public void go(int myParam) {
                    var myLocal = 1;
                    my<caret>
                }
            }
            """.trimIndent(),
        )
        myFixture.completeBasic()
        val items = myFixture.lookupElementStrings ?: emptyList()

        // Scope filtering: only what's reachable from the caret.
        assertContainsElements(items, "myLocal", "myParam", "myField")
        assertDoesntContain(items, "helperLocal")

        // Ordering: local before param before member.
        assertTrue(
            "local above member (got $items)",
            items.indexOf("myLocal") < items.indexOf("myField"),
        )
        assertTrue(
            "param above member (got $items)",
            items.indexOf("myParam") < items.indexOf("myField"),
        )
    }

    fun testOtherClassMembersNotOffered() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Other {
                public int foreignField;
                public void foreignMethod() {}
            }
            public class A {
                public void go() {
                    <caret>
                }
            }
            """.trimIndent(),
        )
        myFixture.completeBasic()
        val items = myFixture.lookupElementStrings ?: emptyList()
        assertDoesntContain(items, "foreignField", "foreignMethod")
        // The class NAME itself is still reachable (e.g. `Other o = …`).
        assertContainsElements(items, "Other")
    }

    fun testValueOfferedInsideSetterBody() {
        val items = completionsAt(
            """
            public class A {
                private int _age;
                public int Age {
                    get -> _age;
                    set { _age = <caret> }
                };
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "value")
    }
}
