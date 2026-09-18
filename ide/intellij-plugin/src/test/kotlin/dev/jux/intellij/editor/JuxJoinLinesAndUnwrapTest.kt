package dev.jux.intellij.editor

import com.intellij.openapi.actionSystem.IdeActions
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Join Lines (Ctrl+Shift+J) and Unwrap/Remove (Ctrl+Shift+Delete), each as
 * the before/after text a user sees.
 */
class JuxJoinLinesAndUnwrapTest : BasePlatformTestCase() {

    private fun join(before: String, after: String) {
        myFixture.configureByText("a.jux", before.trimIndent() + "\n")
        myFixture.performEditorAction(IdeActions.ACTION_EDITOR_JOIN_LINES)
        myFixture.checkResult(after.trimIndent() + "\n")
    }

    /** Apply the unwrap option named [option] at the caret. */
    private fun unwrap(option: String, before: String, after: String) {
        myFixture.configureByText("a.jux", before.trimIndent() + "\n")
        val found = JuxUnwrapDescriptor().collectUnwrappers(project, myFixture.editor, myFixture.file)
        val names = found.map { it.second.getDescription(it.first) }
        val chosen = found.firstOrNull { it.second.getDescription(it.first) == option }
        assertNotNull("'$option' not among $names", chosen)
        WriteCommandAction.runWriteCommandAction(project) {
            chosen!!.second.unwrap(myFixture.editor, chosen.first)
        }
        myFixture.checkResult(after.trimIndent() + "\n")
    }

    // ---- join lines -----------------------------------------------------

    fun testJoinDeclarationAndAssignment() = join(
        """
        void f() {
            int count;<caret>
            count = 5;
        }
        """,
        """
        void f() {
            int count = 5;
        }
        """,
    )

    fun testJoinAdjacentStringLiterals() = join(
        """
        String f() {
            return "Hello, " +<caret>
                "world";
        }
        """,
        """
        String f() {
            return "Hello, world";
        }
        """,
    )

    fun testJoinNestedIfs() = join(
        """
        void f(bool a, bool b) {
            if (a)<caret>
                if (b) {
                    print(1);
                }
        }
        """,
        """
        void f(bool a, bool b) {
            if (a && b) {
                print(1);
            }
        }
        """,
    )

    fun testJoinNestedBracedIfs() = join(
        """
        void f(bool a, bool b) {
            if (a) {<caret>
                if (b) {
                    print(1);
                }
            }
        }
        """,
        """
        void f(bool a, bool b) {
            if (a && b) {
                print(1);
            }
        }
        """,
    )

    // ---- unwrap ---------------------------------------------------------

    fun testUnwrapIf() = unwrap(
        "Unwrap 'if...'",
        """
        void f(bool ready) {
            if (ready) {
                pr<caret>int(1);
                print(2);
            }
            print(3);
        }
        """,
        """
        void f(bool ready) {
            print(1);
            print(2);
            print(3);
        }
        """,
    )

    fun testUnwrapElse() = unwrap(
        "Unwrap 'else...'",
        """
        void f(bool ready) {
            if (ready) {
                print(1);
            } else {
                pr<caret>int(2);
            }
        }
        """,
        """
        void f(bool ready) {
            print(2);
        }
        """,
    )

    fun testRemoveElse() = unwrap(
        "Remove 'else...'",
        """
        void f(bool ready) {
            if (ready) {
                print(1);
            } else {
                pr<caret>int(2);
            }
        }
        """,
        """
        void f(bool ready) {
            if (ready) {
                print(1);
            }
        }
        """,
    )

    fun testUnwrapForEach() = unwrap(
        "Unwrap 'for...'",
        """
        void f(Vec<int> xs) {
            for (var x : xs) {
                pr<caret>int(1);
            }
        }
        """,
        """
        void f(Vec<int> xs) {
            print(1);
        }
        """,
    )

    fun testUnwrapWhile() = unwrap(
        "Unwrap 'while...'",
        """
        void f(int n) {
            while (n > 0) {
                n<caret>--;
            }
        }
        """,
        """
        void f(int n) {
            n--;
        }
        """,
    )

    fun testUnwrapTry() = unwrap(
        "Unwrap 'try...'",
        """
        void f() {
            try {
                wo<caret>rk();
            } catch (Exception e) {
                print(e);
            }
        }
        """,
        """
        void f() {
            work();
        }
        """,
    )

    fun testRemoveOneOfTwoCatches() = unwrap(
        "Remove 'catch...'",
        """
        void f() {
            try {
                work();
            } catch (IOException e) {
                pr<caret>int(1);
            } catch (Exception e) {
                print(2);
            }
        }
        """,
        """
        void f() {
            try {
                work();
            } catch (Exception e) {
                print(2);
            }
        }
        """,
    )

    fun testRemoveFinallyKeepsCatch() = unwrap(
        "Remove 'finally...'",
        """
        void f() {
            try {
                work();
            } catch (Exception e) {
                print(1);
            } finally {
                do<caret>ne();
            }
        }
        """,
        """
        void f() {
            try {
                work();
            } catch (Exception e) {
                print(1);
            }
        }
        """,
    )

    fun testUnwrapBraces() = unwrap(
        "Unwrap braces",
        """
        void f() {
            {
                pr<caret>int(1);
            }
        }
        """,
        """
        void f() {
            print(1);
        }
        """,
    )

    fun testUnwrapLambda() = unwrap(
        "Unwrap lambda...",
        """
        void f() {
            run(() -> {
                wo<caret>rk();
            });
        }
        """,
        """
        void f() {
            work();
        }
        """,
    )
}
