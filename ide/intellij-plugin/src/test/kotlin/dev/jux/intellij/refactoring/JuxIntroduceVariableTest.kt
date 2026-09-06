package dev.jux.intellij.refactoring

import com.intellij.codeInsight.template.impl.TemplateManagerImpl
import com.intellij.refactoring.util.CommonRefactoringUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Extract Variable (`Ctrl+Alt+V`).
 *
 * Each test asserts the document the user is left with, since that is the whole
 * product of the refactoring. The generated name is `value` (suffixed when
 * taken), which the in-place rename template then invites you to replace.
 */
class JuxIntroduceVariableTest : BasePlatformTestCase() {

    fun testHoistsTheSelectedExpression() {
        val after = extract(
            """
            class C {
                void m() {
                    print(<selection>a + b</selection>);
                }
            }
            """.trimIndent(),
        )
        assertTrue("expected a `var` declaration, got:\n$after", after.contains("var value = a + b;"))
        assertTrue("expected the call to use it, got:\n$after", after.contains("print(value);"))
    }

    fun testReplacesEveryOccurrence() {
        val after = extract(
            """
            class C {
                void m() {
                    print(<selection>a + b</selection>);
                    print(a + b);
                }
            }
            """.trimIndent(),
        )
        assertEquals("both calls should use the new binding:\n$after", 2, after.split("print(value);").size - 1)
        assertFalse("no occurrence should be left behind:\n$after", after.contains("print(a + b)"))
    }

    fun testTheDeclarationGoesBeforeTheFirstOccurrence() {
        val after = extract(
            """
            class C {
                void m() {
                    print(a + b);
                    print(<selection>a + b</selection>);
                }
            }
            """.trimIndent(),
        )
        assertTrue(
            "the declaration must precede every use:\n$after",
            after.indexOf("var value = a + b;") < after.indexOf("print(value);"),
        )
    }

    fun testTheGeneratedNameDoesNotShadowAnExistingBinding() {
        val after = extract(
            """
            class C {
                void m() {
                    var value = 0;
                    print(<selection>a + b</selection>);
                }
            }
            """.trimIndent(),
        )
        assertTrue("expected a suffixed name, got:\n$after", after.contains("var value1 = a + b;"))
    }

    fun testAnAssignmentIsRefused() {
        // Hoisting a write would move when it happens, which is a behaviour
        // change rather than a refactoring. The platform turns the error hint
        // into an exception under test, which is how the refusal is observed.
        val message = refusalOf(
            """
            class C {
                void m() { <selection>x = 1</selection>; }
            }
            """.trimIndent(),
        )
        assertTrue("the message should say why: $message", message.contains("assignment"))
    }

    /** The refusal message for a source the handler declines to touch. */
    private fun refusalOf(source: String): String {
        myFixture.configureByText("a.jux", source)
        val before = myFixture.editor.document.text
        try {
            JuxIntroduceVariableHandler().invoke(project, myFixture.editor, myFixture.file, null)
        } catch (e: CommonRefactoringUtil.RefactoringErrorHintException) {
            assertEquals("a refused refactoring must not touch the file", before, myFixture.editor.document.text)
            return e.message ?: ""
        }
        throw AssertionError("expected the refactoring to be refused, but it ran")
    }

    private fun extract(source: String): String {
        myFixture.configureByText("a.jux", source)
        TemplateManagerImpl.setTemplateTesting(testRootDisposable)
        JuxIntroduceVariableHandler().invoke(project, myFixture.editor, myFixture.file, null)
        // Close the in-place rename so the document settles on the generated
        // name; the template itself is the platform's, not ours to assert.
        TemplateManagerImpl.getTemplateState(myFixture.editor)?.gotoEnd(false)
        return myFixture.editor.document.text
    }
}
