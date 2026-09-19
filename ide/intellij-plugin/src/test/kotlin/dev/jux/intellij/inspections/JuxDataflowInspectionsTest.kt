package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The final-candidate, never-read and nullability inspections added in 0.1.0:
 * what each reports, what it must leave alone, and what its fix writes.
 */
class JuxDataflowInspectionsTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxFieldMayBeFinalInspection(),
            JuxLocalMayBeFinalInspection(),
            JuxTypeCanBeVarInspection(),
            JuxAssignedNeverReadInspection(),
            JuxRedundantNullCheckInspection(),
            JuxNullableAccessInspection(),
        )
    }

    private fun descriptions(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    private fun applyFix(code: String, fix: String): String {
        myFixture.configureByText("a.jux", code.trimIndent())
        myFixture.doHighlighting()
        val action = myFixture.getAllQuickFixes().firstOrNull { it.text == fix }
            ?: error("no fix '$fix' among ${myFixture.getAllQuickFixes().map { it.text }}")
        myFixture.launchAction(action)
        return myFixture.editor.document.text
    }

    // ---- field may be final -------------------------------------------------

    fun testFieldSetOnlyInConstructorMayBeFinal() {
        val d = descriptions(
            """
            class Account {
                private String owner;
                private int balance = 0;
                private int opened = 1;

                Account(String owner) {
                    this.owner = owner;
                }

                void deposit(int n) {
                    balance += n;
                }

                int age() { return opened; }
            }
            """,
        )
        assertTrue(d.toString(), "Field 'owner' may be 'final'" in d)
        assertTrue(d.toString(), "Field 'opened' may be 'final'" in d)
        assertFalse(d.toString(), d.any { it.contains("'balance'") && it.contains("final") })
    }

    fun testFieldAssignedInABranchOrTwiceIsLeftAlone() {
        val d = descriptions(
            """
            class Box {
                private int size;
                private int kind;
                private int shade;

                Box(bool big) {
                    if (big) { size = 10; } else { size = 1; }
                    kind = 1;
                    kind = 2;
                }

                Box() {
                    this(false);
                }

                void paint() { shade = 3; }
            }
            """,
        )
        assertFalse(d.toString(), d.any { it.endsWith("may be 'final'") })
    }

    fun testNonPrivateAndStaticAndPropertyFieldsAreLeftAlone() {
        val d = descriptions(
            """
            class C {
                public int a = 1;
                private static int b = 2;
                private int C1 { get; set; } = 3;
            }
            """,
        )
        assertFalse(d.toString(), d.any { it.endsWith("may be 'final'") })
    }

    fun testMakeFieldFinalFix() {
        val after = applyFix(
            """
            class Account {
                private String owner;
                Account(String owner) {
                    this.owner = owner;
                }
            }
            """,
            "Make 'final'",
        )
        assertTrue(after, after.contains("private final String owner;"))
    }

    // ---- local may be final / can be var -------------------------------------

    fun testLocalMayBeFinalOnlyWhenNeverReassigned() {
        val d = descriptions(
            """
            void f() {
                var total = 3;
                var count = 0;
                count++;
                final var fixed = 1;
                print(total + count + fixed);
            }
            """,
        )
        assertTrue(d.toString(), "Variable 'total' can be 'final'" in d)
        assertFalse(d.toString(), "Variable 'count' can be 'final'" in d)
        assertFalse(d.toString(), "Variable 'fixed' can be 'final'" in d)
    }

    fun testTypeCanBeVarOnlyWhenTheInitializerSpellsIt() {
        val d = descriptions(
            """
            class Point { Point(int x, int y) {} }
            void f() {
                Point p = new Point(1, 2);
                Vec<int> v = new Vec<>();
                Vec<int> w = new Vec<int>();
                print(p);
                print(v);
                print(w);
            }
            """,
        )
        assertEquals(d.toString(), 2, d.count { it == "Explicit type can be replaced with 'var'" })
        val after = applyFix(
            "class Point { Point(int x, int y) {} }\nvoid f() {\n    Point p = new Point(1, 2);\n    print(p);\n}",
            "Replace with 'var'",
        )
        assertTrue(after, after.contains("var p = new Point(1, 2);"))
    }

    // ---- assigned but never read ---------------------------------------------

    fun testAssignedButNeverRead() {
        val d = descriptions(
            """
            void f(int[] xs) {
                var last = 0;
                for (var x : xs) {
                    last = x;
                }
                var shown = 1;
                shown = 2;
                print(${'$'}"${'$'}{shown}");
                var counter = 0;
                counter += 1;
            }
            """,
        )
        assertTrue(d.toString(), "Variable 'last' is assigned but never read" in d)
        assertFalse(d.toString(), d.any { it.startsWith("Variable 'shown' is assigned") })
        assertFalse(d.toString(), d.any { it.startsWith("Variable 'counter' is assigned") })
    }

    // ---- redundant null check -------------------------------------------------

    fun testNullCheckOnNonNullableIsReported() {
        val d = descriptions(
            """
            void greet(String name, String? nick, int n) {
                if (name != null) { print(name); }
                if (nick != null) { print(nick); }
                if (n == null) { print(n); }
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.startsWith("Condition 'name != null' is always true") })
        assertTrue(d.toString(), d.any { it.startsWith("Condition 'n == null' is always false") })
        assertFalse(d.toString(), d.any { it.contains("'nick") && it.startsWith("Condition") })
        val after = applyFix("void g(String name) {\n    if (name != null) { print(name); }\n}", "Replace with 'true'")
        assertTrue(after, after.contains("if (true)"))
    }

    fun testTypeParameterIsNotAssumedNonNull() {
        val d = descriptions(
            """
            class Holder<T> {
                private T value;
                Holder(T value) { this.value = value; }
                bool empty(T probe) { return probe == null; }
            }
            // A raw pointer may be null: testing it is how it is used.
            int count(int* p) { return p != null ? 1 : 0; }
            """,
        )
        assertFalse(d.toString(), d.any { it.startsWith("Condition") })
    }

    // ---- nullable used without a check ---------------------------------------

    fun testNullableMemberUseWithoutAnyCheckIsReported() {
        val d = descriptions(
            """
            class Node { public int value; }
            void show(Node? n) {
                print(n.value);
            }
            void safe(Node? n) {
                if (n == null) { return; }
                print(n.value);
            }
            void asserted(Node? n) {
                print(n!!.value);
            }
            void defaulted(Node? n, Node d) {
                var m = n ?: d;
                print(m.value);
            }
            """,
        )
        assertEquals(d.toString(), 1, d.count { it.startsWith("'n' may be null") })
        val after = applyFix("class Node { public int value; }\nvoid show(Node? n) {\n    print(n.value);\n}", "Assert non-null with '!!'")
        assertTrue(after, after.contains("n!!.value"))
    }
}
