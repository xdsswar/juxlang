package dev.jux.intellij.refactoring

import com.intellij.psi.util.PsiTreeUtil
import com.intellij.refactoring.util.CommonRefactoringUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.psi.JuxLocalVariable

/**
 * Inline Variable (`Ctrl+Alt+N`).
 *
 * The refusals matter as much as the substitutions: each of the three guarded
 * cases would change what the program does, not just how it reads.
 */
class JuxInlineVariableTest : BasePlatformTestCase() {

    private val handler = JuxInlineVariableHandler()

    fun testItIsOfferedForALocalAndForNothingElse() {
        assertTrue(handler.isEnabledForLanguage(JuxLanguage))
        myFixture.configureByText("a.jux", "class C { void m() { var x = 1; print(x); } }")
        assertTrue(handler.canInlineElement(localNamed("x")))
        assertFalse(handler.canInlineElement(myFixture.file))
    }

    fun testSubstitutesTheInitializerAndDropsTheDeclaration() {
        val after = inline(
            """
            class C {
                void m() {
                    var x = compute();
                    print(x);
                }
            }
            """.trimIndent(),
            "x",
        )
        assertTrue("expected the initializer at the use site:\n$after", after.contains("print(compute());"))
        assertFalse("the declaration should be gone:\n$after", after.contains("var x ="))
    }

    fun testEveryUseIsSubstituted() {
        val after = inline(
            """
            class C {
                void m() {
                    var x = value();
                    print(x);
                    print(x);
                }
            }
            """.trimIndent(),
            "x",
        )
        assertEquals("both uses should be inlined:\n$after", 2, after.split("print(value());").size - 1)
    }

    fun testACompoundInitializerIsParenthesized() {
        // `var d = a + b; … d * 2` must not become `a + b * 2`.
        val after = inline(
            """
            class C {
                void m() {
                    var d = a + b;
                    print(d * 2);
                }
            }
            """.trimIndent(),
            "d",
        )
        assertTrue("expected parentheses around the sum:\n$after", after.contains("print((a + b) * 2);"))
    }

    fun testAnAtomicInitializerIsNotParenthesized() {
        val after = inline(
            """
            class C {
                void m() {
                    var n = 42;
                    print(n);
                }
            }
            """.trimIndent(),
            "n",
        )
        assertTrue("a literal needs no brackets:\n$after", after.contains("print(42);"))
    }

    fun testAReassignedVariableIsRefused() {
        val message = refusalOf(
            """
            class C {
                void m() {
                    var x = 1;
                    x = 2;
                    print(x);
                }
            }
            """.trimIndent(),
            "x",
        )
        assertTrue("the message should say why: $message", message.contains("reassigned"))
    }

    fun testAVariableWithNoInitializerIsRefused() {
        val message = refusalOf(
            """
            class C {
                void m() {
                    int x;
                    print(x);
                }
            }
            """.trimIndent(),
            "x",
        )
        assertTrue("the message should say why: $message", message.contains("no initializer"))
    }

    /**
     * The refusal message for a local the handler declines to inline. The
     * platform turns an error hint into an exception under test, which is how
     * the refusal is observed -- and the file must be untouched either way.
     */
    private fun refusalOf(source: String, name: String): String {
        myFixture.configureByText("a.jux", source)
        val before = myFixture.editor.document.text
        try {
            handler.inlineElement(project, myFixture.editor, localNamed(name))
        } catch (e: CommonRefactoringUtil.RefactoringErrorHintException) {
            assertEquals("a refused refactoring must not touch the file", before, myFixture.editor.document.text)
            return e.message ?: ""
        }
        throw AssertionError("expected the inline to be refused, but it ran")
    }

    private fun inline(source: String, name: String): String {
        myFixture.configureByText("a.jux", source)
        handler.inlineElement(project, myFixture.editor, localNamed(name))
        return myFixture.editor.document.text
    }

    private fun localNamed(name: String): JuxLocalVariable =
        PsiTreeUtil.findChildrenOfType(myFixture.file, JuxLocalVariable::class.java)
            .first { it.name == name }
}
