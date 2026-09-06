package dev.jux.intellij.editor

import com.intellij.openapi.actionSystem.IdeActions
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * [JuxSmartEnterProcessor] — Complete Current Statement (`Ctrl+Shift+Enter`).
 *
 * Runs the real editor action, so what is asserted is the document the user
 * would be looking at. Every fixture is laid out over several lines because the
 * processor completes the statement *at the caret*, and the shape of the line
 * it is on is precisely what it reads.
 */
class JuxSmartEnterTest : BasePlatformTestCase() {

    fun testTerminatesAStatement() {
        val after = completeStatement(
            """
            class C {
                void m() {
                    var x = 1<caret>
                }
            }
            """.trimIndent(),
        )
        assertTrue("expected a `;` after the initializer, got:\n$after", after.contains("var x = 1;"))
    }

    fun testClosesOpenParentheses() {
        val after = completeStatement(
            """
            class C {
                void m() {
                    print(greet(name<caret>
                }
            }
            """.trimIndent(),
        )
        assertTrue("expected both parens closed, got:\n$after", after.contains("print(greet(name));"))
    }

    fun testClosesAnUnterminatedString() {
        val after = completeStatement(
            """
            class C {
                void m() {
                    print("hi<caret>
                }
            }
            """.trimIndent(),
        )
        assertTrue("expected the quote and paren closed, got:\n$after", after.contains("print(\"hi\");"))
    }

    fun testGivesAnIfHeaderABody() {
        val after = completeStatement(
            """
            class C {
                void m() {
                    if (x > 0)<caret>
                }
            }
            """.trimIndent(),
        )
        assertTrue("expected a brace body, got:\n$after", after.contains("if (x > 0) {"))
        assertFalse("a header must not be terminated with a semicolon:\n$after", after.contains("if (x > 0);"))
    }

    fun testAnUnclosedIfHeaderIsClosedThenGivenABody() {
        val after = completeStatement(
            """
            class C {
                void m() {
                    if (x > 0<caret>
                }
            }
            """.trimIndent(),
        )
        assertTrue("expected the paren closed and a body opened, got:\n$after", after.contains("if (x > 0) {"))
    }

    fun testAMemberSignatureGetsABodyNotASemicolon() {
        // The same `…)` shape means different things in a class body and in a
        // method body; only the class-body one wants braces.
        val after = completeStatement(
            """
            class C {
                public int area()<caret>
            }
            """.trimIndent(),
        )
        assertTrue("expected a method body, got:\n$after", after.contains("public int area() {"))
    }

    fun testACallStatementStillGetsItsSemicolon() {
        val after = completeStatement(
            """
            class C {
                void m() {
                    doWork()<caret>
                }
            }
            """.trimIndent(),
        )
        assertTrue("expected a `;`, got:\n$after", after.contains("doWork();"))
        assertFalse("a call is not a declaration:\n$after", after.contains("doWork() {"))
    }

    fun testAnAlreadyCompleteStatementIsLeftAlone() {
        val after = completeStatement(
            """
            class C {
                void m() {
                    var x = 1;<caret>
                }
            }
            """.trimIndent(),
        )
        assertFalse("expected no doubled semicolon, got:\n$after", after.contains("var x = 1;;"))
    }

    private fun completeStatement(source: String): String {
        myFixture.configureByText("a.jux", source)
        myFixture.performEditorAction(IdeActions.ACTION_EDITOR_COMPLETE_STATEMENT)
        return myFixture.editor.document.text
    }
}
