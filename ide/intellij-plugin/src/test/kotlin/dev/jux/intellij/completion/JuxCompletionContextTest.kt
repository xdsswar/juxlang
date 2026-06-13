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

    fun testAfterDotOnInFileTypeOffersItsMembers() {
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
        // `other` is an `A` → the fallback now infers the type in-file and
        // offers A's members (was LSP-only / empty before the inference pass).
        assertContainsElements(items, "plain", "go")
    }

    fun testAfterDotOnUnresolvableReceiverStaysEmpty() {
        val items = completionsAt(
            """
            public class A {
                public void go(SomeStdType other) {
                    other.<caret>
                }
            }
            """.trimIndent(),
        )
        // SomeStdType isn't a project type — member completion can't infer it,
        // so the fallback stays empty (the LSP owns std/crate members).
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

    // ---- testing framework (sec. TS) + newer surface ---------------------------

    fun testAnnotationNamesAfterAt() {
        val items = completionsAt(
            """
            package demo;
            @<caret>
            void f() {}
            """.trimIndent(),
        )
        assertContainsElements(items, "override", "Test", "BeforeEach", "AfterEach", "BeforeAll", "AfterAll")
        // Only the builtin annotations belong after `@` — no keywords/types.
        assertDoesntContain(items, "class", "public", "return")
    }

    fun testForAwaitOfferedInStatementPosition() {
        val items = completionsAt(
            """
            public class A {
                public async void go() {
                    <caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "for", "for await")
    }

    // ---- member completion after a dot (in-file type inference) ---------------

    fun testInstanceMembersAfterDotOnNewLocal() {
        // `var p = new Point();` → p is a Point → its instance members complete.
        val items = completionsAt(
            """
            package demo;
            public class Point {
                public int x;
                public int y;
                public int manhattan() { return this.x + this.y; }
                public static Point origin() { return new Point(); }
            }
            public class App {
                public void go() {
                    var p = new Point();
                    p.<caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "x", "y", "manhattan")
        // Static members are NOT offered on an instance receiver.
        assertDoesntContain(items, "origin")
    }

    fun testInstanceMembersFromDeclaredType() {
        val items = completionsAt(
            """
            package demo;
            public class Engine { public void start() {} public int rpm; }
            public class Car {
                public void drive(Engine e) {
                    e.<caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "start", "rpm")
    }

    fun testStaticMembersAndEnumConstantsAfterDotOnType() {
        val items = completionsAt(
            """
            package demo;
            public enum Color { Red, Green, Blue }
            public class App {
                public void go() {
                    Color.<caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "Red", "Green", "Blue")
    }

    fun testThisOffersEnclosingMembers() {
        val items = completionsAt(
            """
            package demo;
            public class Widget {
                private int width;
                public void resize() {}
                public void run() {
                    this.<caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "width", "resize")
    }

    fun testInheritedMembersAfterDot() {
        val items = completionsAt(
            """
            package demo;
            public class Base { public void shared() {} }
            public class Derived extends Base { public void own() {} }
            public class App {
                public void go(Derived d) {
                    d.<caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "own", "shared")
    }

    // ---- cross-file type discovery + auto-import ------------------------------

    fun testCrossFileTypeIsDiscoverableAndAutoImports() {
        myFixture.addFileToProject(
            "lib/Widget.jux",
            """
            package lib;
            public class Widget { public void render() {} }
            """.trimIndent(),
        )
        myFixture.configureByText(
            "App.jux",
            """
            package app;
            public class App {
                public void go() {
                    var w = new Wid<caret>
                }
            }
            """.trimIndent(),
        )
        val items = myFixture.completeBasic()
        // Either the lookup auto-inserted (single match) or Widget is listed.
        val strings = myFixture.lookupElementStrings
        if (strings != null) {
            assertTrue("cross-file Widget discoverable", strings.contains("Widget"))
        }
        // After accepting Widget, the import lands.
        if (strings == null || strings.size == 1) {
            assertTrue("auto-import inserted", myFixture.file.text.contains("import lib.Widget;"))
        }
    }

    fun testLiteralConstantsCompleteInExpressionPosition() {
        val items = completionsAt(
            """
            public class A {
                public void go() {
                    var x = <caret>
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "null", "true", "false")
    }

    fun testTypeofOfferedExactlyWhenReserved() {
        // Forward-compat (parallel compiler session): the entry must appear in
        // expression completion exactly when the generated alphabet has it —
        // this passes both before and after `typeof` lands in jux-tokens.json.
        val items = completionsAt(
            """
            public class A {
                public void go() {
                    var x = <caret>
                }
            }
            """.trimIndent(),
        )
        val reserved = "typeof" in dev.jux.intellij.highlight.JuxKeywords.KEYWORDS
        assertEquals("typeof offered iff reserved (got $items)", reserved, "typeof" in items)
    }

    // ---- completion inside `${…}` interpolation holes -------------------------

    private val D = '$' // keeps Kotlin string interpolation out of the snippets

    fun testInterpolationHoleOffersVisibleDeclarations() {
        val items = completionsAt(
            """
            package demo;
            public class A {
                private int width;
                public void go(int param) {
                    var local = 1;
                    var s = ${D}"x=${D}{ <caret> }";
                }
            }
            """.trimIndent(),
        )
        // An expression hole sees locals, params and enclosing members…
        assertContainsElements(items, "local", "param", "width")
        // …but not statement keywords (it's an expression, not a statement).
        assertDoesntContain(items, "return", "class", "while")
    }

    fun testInterpolationHoleMemberAccess() {
        val items = completionsAt(
            """
            package demo;
            public class Point { public int x; public int y; }
            public class A {
                public void go() {
                    var p = new Point();
                    var s = ${D}"v=${D}{ p.<caret> }";
                }
            }
            """.trimIndent(),
        )
        assertContainsElements(items, "x", "y")
    }

    fun testInterpolationPlainTextOffersNothing() {
        // Caret in the literal text of an interpolation string — outside any
        // hole — must NOT trigger code completion (that would be noise).
        val items = completionsAt(
            """
            public class A {
                public void go(int param) {
                    var s = ${D}"hello <caret>";
                }
            }
            """.trimIndent(),
        )
        assertDoesntContain(items, "param")
    }

    fun testPlainStringTextOffersNothing() {
        val items = completionsAt(
            """
            public class A {
                public void go(int param) {
                    var s = "hello <caret>";
                }
            }
            """.trimIndent(),
        )
        assertDoesntContain(items, "param", "go")
    }
}
