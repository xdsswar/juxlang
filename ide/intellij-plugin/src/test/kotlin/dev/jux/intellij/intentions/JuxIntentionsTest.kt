package dev.jux.intellij.intentions

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The Jux Alt+Enter intentions, each as the before/after text a user sees,
 * plus the places each must NOT be offered.
 */
class JuxIntentionsTest : BasePlatformTestCase() {

    private fun doTest(intention: String, before: String, after: String) {
        myFixture.configureByText("a.jux", before.trimIndent() + "\n")
        myFixture.launchAction(myFixture.findSingleIntention(intention))
        myFixture.checkResult(after.trimIndent() + "\n")
    }

    private fun assertNotOffered(intention: String, text: String) {
        myFixture.configureByText("a.jux", text.trimIndent() + "\n")
        assertEmpty(myFixture.filterAvailableIntentions(intention))
    }

    /**
     * Every Jux intention ships its Settings | Editor | Intentions page: a
     * description and a before/after example. A missing one is a blank page.
     */
    fun testEveryIntentionHasItsDescription() {
        val jux = com.intellij.codeInsight.intention.IntentionManager.getInstance().availableIntentions
            .map { com.intellij.codeInsight.intention.IntentionActionDelegate.unwrap(it) }
            .filter { it.javaClass.name.startsWith("dev.jux.intellij.intentions.Jux") && it is JuxIntention }
        assertTrue("no Jux intentions registered", jux.size >= 15)
        for (intention in jux) {
            val dir = "intentionDescriptions/${intention.javaClass.simpleName}"
            for (file in listOf("description.html", "before.jux.template", "after.jux.template")) {
                assertNotNull("$dir/$file missing", javaClass.classLoader.getResource("$dir/$file"))
            }
        }
    }

    /** The Alt+Enter preview renders the same result the intention writes. */
    fun testPreviewMatchesResult() {
        myFixture.configureByText(
            "a.jux",
            "void f(int a, int b) {\n    <caret>if (a == b) {\n        print(1);\n    }\n}\n",
        )
        val action = myFixture.findSingleIntention("Invert 'if' condition")
        val preview = myFixture.getIntentionPreviewText(action)
        myFixture.launchAction(action)
        assertEquals(myFixture.editor.document.text, preview)
    }

    // ---- invert if ------------------------------------------------------

    fun testInvertIfSwapsBranches() = doTest(
        "Invert 'if' condition",
        """
        void f(int a, int b) {
            <caret>if (a == b) {
                print(1);
            } else {
                print(2);
            }
        }
        """,
        """
        void f(int a, int b) {
            if (a != b) {
                print(2);
            } else {
                print(1);
            }
        }
        """,
    )

    fun testInvertIfWithoutElseDropsDoubleNegation() = doTest(
        "Invert 'if' condition",
        """
        void f(bool ready) {
            if (!<caret>ready) {
                go();
            }
        }
        """,
        """
        void f(bool ready) {
            if (ready) {
            } else {
                go();
            }
        }
        """,
    )

    fun testInvertIfNotOfferedInBranch() = assertNotOffered(
        "Invert 'if' condition",
        """
        void f(bool ready) {
            if (ready) {
                go<caret>();
            }
        }
        """,
    )

    // ---- flip -----------------------------------------------------------

    fun testFlipMirrorsRelational() = doTest(
        "Flip '<' to '>'",
        """
        bool f(int a, int b) {
            return a <caret>< b;
        }
        """,
        """
        bool f(int a, int b) {
            return b > a;
        }
        """,
    )

    fun testFlipEquality() = doTest(
        "Flip '=='",
        """
        bool f(int a, int b) {
            return a + 1 <caret>== b;
        }
        """,
        """
        bool f(int a, int b) {
            return b == a + 1;
        }
        """,
    )

    // ---- split / join declaration --------------------------------------

    fun testSplitDeclaration() = doTest(
        "Split into declaration and assignment",
        """
        void f() {
            int <caret>count = 5;
            print(count);
        }
        """,
        """
        void f() {
            int count;
            count = 5;
            print(count);
        }
        """,
    )

    fun testSplitVarDeclarationWritesTheType() = doTest(
        "Split into declaration and assignment",
        """
        void f() {
            var <caret>count = 5;
        }
        """,
        """
        void f() {
            int count;
            count = 5;
        }
        """,
    )

    fun testJoinDeclaration() = doTest(
        "Join declaration and assignment",
        """
        void f() {
            int <caret>count;
            count = 5;
            print(count);
        }
        """,
        """
        void f() {
            int count = 5;
            print(count);
        }
        """,
    )

    fun testJoinNotOfferedWhenValueReadsTheVariable() = assertNotOffered(
        "Join declaration and assignment",
        """
        void f() {
            int <caret>count;
            count = count + 1;
        }
        """,
    )

    // ---- braces ---------------------------------------------------------

    fun testAddBracesToIf() = doTest(
        "Add braces to 'if' statement",
        """
        void f(bool ready) {
            <caret>if (ready) print("go");
        }
        """,
        """
        void f(bool ready) {
            if (ready) {
                print("go");
            }
        }
        """,
    )

    fun testAddBracesToElse() = doTest(
        "Add braces to 'else' statement",
        """
        void f(bool ready) {
            if (ready) {
                print("go");
            } <caret>else print("wait");
        }
        """,
        """
        void f(bool ready) {
            if (ready) {
                print("go");
            } else {
                print("wait");
            }
        }
        """,
    )

    fun testAddBracesToWhile() = doTest(
        "Add braces to 'while' statement",
        """
        void f(int n) {
            <caret>while (n > 0) n--;
        }
        """,
        """
        void f(int n) {
            while (n > 0) {
                n--;
            }
        }
        """,
    )

    fun testRemoveBraces() = doTest(
        "Remove braces from 'for' statement",
        """
        void f(int n) {
            <caret>for (int i = 0; i < n; i++) {
                print(i);
            }
        }
        """,
        """
        void f(int n) {
            for (int i = 0; i < n; i++) print(i);
        }
        """,
    )

    fun testRemoveBracesNotOfferedWhereElseWouldMove() = assertNotOffered(
        "Remove braces from 'if' statement",
        """
        void f(bool a, bool b) {
            <caret>if (a) {
                if (b) print(1);
            } else print(2);
        }
        """,
    )

    // ---- interpolation --------------------------------------------------

    fun testConcatToInterpolation() = doTest(
        "Replace '+' with string interpolation",
        """
        String f(String name, int n) {
            return "Hello, " <caret>+ name + "! You have " + (n + 1) + " items";
        }
        """,
        """
        String f(String name, int n) {
            return ${'$'}"Hello, ${'$'}{name}! You have ${'$'}{n + 1} items";
        }
        """,
    )

    fun testConcatNotOfferedWhenNumbersAddFirst() = assertNotOffered(
        "Replace '+' with string interpolation",
        """
        String f(int a, int b) {
            return a <caret>+ b + " total";
        }
        """,
    )

    fun testInterpolationToConcat() = doTest(
        "Replace string interpolation with '+'",
        """
        String f(String name, int a, int b) {
            return <caret>${'$'}"Hi ${'$'}{name}, sum ${'$'}{a + b}";
        }
        """,
        """
        String f(String name, int a, int b) {
            return "Hi " + name + ", sum " + (a + b);
        }
        """,
    )

    fun testInterpolationToConcatStartsWithTextWhenValuesLead() = doTest(
        "Replace string interpolation with '+'",
        """
        String f(int a, int b) {
            return <caret>${'$'}"${'$'}{a}${'$'}{b}";
        }
        """,
        """
        String f(int a, int b) {
            return "" + a + b;
        }
        """,
    )

    // ---- ternary / if ---------------------------------------------------

    fun testTernaryToIfInReturn() = doTest(
        "Replace '?:' with 'if else'",
        """
        int f(bool c) {
            return c <caret>? 1 : 2;
        }
        """,
        """
        int f(bool c) {
            if (c) {
                return 1;
            } else {
                return 2;
            }
        }
        """,
    )

    fun testTernaryToIfInDeclaration() = doTest(
        "Replace '?:' with 'if else'",
        """
        void f(bool c) {
            var n = c <caret>? 1 : 2;
            print(n);
        }
        """,
        """
        void f(bool c) {
            int n;
            if (c) {
                n = 1;
            } else {
                n = 2;
            }
            print(n);
        }
        """,
    )

    fun testIfToTernaryAssignment() = doTest(
        "Replace 'if else' with '?:'",
        """
        void f(bool c) {
            int n = 0;
            <caret>if (c) {
                n = 1;
            } else {
                n = 2;
            }
        }
        """,
        """
        void f(bool c) {
            int n = 0;
            n = c ? 1 : 2;
        }
        """,
    )

    fun testIfToTernaryReturn() = doTest(
        "Replace 'if else' with '?:'",
        """
        int f(bool c) {
            <caret>if (c) return 1; else return 2;
        }
        """,
        """
        int f(bool c) {
            return c ? 1 : 2;
        }
        """,
    )

    // ---- nested ifs -----------------------------------------------------

    fun testMergeNestedIfs() = doTest(
        "Merge nested 'if's",
        """
        void f(bool a, bool b, bool c) {
            <caret>if (a || b) {
                if (c) {
                    print(1);
                }
            }
        }
        """,
        """
        void f(bool a, bool b, bool c) {
            if ((a || b) && c) {
                print(1);
            }
        }
        """,
    )

    fun testSplitAndIntoIfs() = doTest(
        "Split into 2 'if' statements",
        """
        void f(bool a, bool b, bool c) {
            if (a && b <caret>&& c) {
                print(1);
            }
        }
        """,
        """
        void f(bool a, bool b, bool c) {
            if (a && b) {
                if (c) {
                    print(1);
                }
            }
        }
        """,
    )

    fun testSplitFirstAndIntoIfs() = doTest(
        "Split into 2 'if' statements",
        """
        void f(bool a, bool b, bool c) {
            if (a <caret>&& b && c) {
                print(1);
            }
        }
        """,
        """
        void f(bool a, bool b, bool c) {
            if (a) {
                if (b && c) {
                    print(1);
                }
            }
        }
        """,
    )

    fun testMergeElseIf() = doTest(
        "Merge 'else if'",
        """
        void f(bool a, bool b) {
            if (a) {
                print(1);
            } <caret>else {
                if (b) {
                    print(2);
                }
            }
        }
        """,
        """
        void f(bool a, bool b) {
            if (a) {
                print(1);
            } else if (b) {
                print(2);
            }
        }
        """,
    )

    // ---- var / explicit type -------------------------------------------

    fun testVarToExplicitType() = doTest(
        "Replace 'var' with explicit type",
        """
        void f() {
            <caret>var n = 5;
        }
        """,
        """
        void f() {
            int n = 5;
        }
        """,
    )

    fun testExplicitTypeToVar() = doTest(
        "Replace explicit type with 'var'",
        """
        void f() {
            <caret>String s = "hi";
        }
        """,
        """
        void f() {
            var s = "hi";
        }
        """,
    )

    fun testExplicitTypeToVarNotOfferedWhenTypeWouldChange() = assertNotOffered(
        "Replace explicit type with 'var'",
        """
        void f() {
            <caret>double d = 1;
        }
        """,
    )
}
