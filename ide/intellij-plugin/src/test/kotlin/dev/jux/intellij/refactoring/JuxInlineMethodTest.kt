package dev.jux.intellij.refactoring

import com.intellij.lang.refactoring.InlineActionHandler
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.refactoring.util.CommonRefactoringUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxMethodDeclaration

/**
 * Inline Method (`Ctrl+Alt+N` on a method): calls become the body, arguments
 * substitute where that is safe and go into locals where it is not, and the
 * method is deleted.
 */
class JuxInlineMethodTest : BasePlatformTestCase() {

    fun testASingleReturnMethodBecomesItsExpression() {
        doTest(
            """
            class Calc {
                int twice(int x) {
                    return x * 2;
                }

                int use(int a) {
                    return twice(a) + 1;
                }
            }
            """.trimIndent(),
            "twice",
            """
            class Calc {
                int use(int a) {
                    return (a * 2) + 1;
                }
            }
            """.trimIndent(),
        )
    }

    fun testAVoidMethodBecomesItsStatements() {
        doTest(
            """
            void greet(String name) {
                print("hi");
                print(${'$'}"hello ${'$'}{name}");
            }

            void main() {
                greet("ann");
            }
            """.trimIndent(),
            "greet",
            """
            void main() {
                print("hi");
                print(${'$'}"hello ${'$'}{"ann"}");
            }
            """.trimIndent(),
        )
    }

    fun testAnArgumentWithACallGoesIntoALocal() {
        doTest(
            """
            int next() {
                return 1;
            }

            int sq(int v) {
                return v * v;
            }

            void main() {
                print(sq(next()));
            }
            """.trimIndent(),
            "sq",
            """
            int next() {
                return 1;
            }

            void main() {
                int v = next();
                print(v * v);
            }
            """.trimIndent(),
        )
    }

    fun testALocalThatCollidesIsRenamed() {
        doTest(
            """
            int add(int a, int b) {
                int sum = a + b;
                return sum;
            }

            void main() {
                int sum = 5;
                var total = add(sum, 2);
                print(total);
            }
            """.trimIndent(),
            "add",
            """
            void main() {
                int sum = 5;
                int sum1 = sum + 2;
                var total = sum1;
                print(total);
            }
            """.trimIndent(),
        )
    }

    fun testRecursionIsRefused() {
        assertRefused(
            """
            int fact(int n) {
                return n <= 1 ? 1 : n * fact(n - 1);
            }

            void main() { print(fact(3)); }
            """.trimIndent(),
            "fact",
            "calls itself",
        )
    }

    fun testAnEarlyReturnIsRefused() {
        assertRefused(
            """
            int sign(int n) {
                if (n < 0) {
                    return -1;
                }
                return 1;
            }

            void main() { print(sign(3)); }
            """.trimIndent(),
            "sign",
            "more than one place",
        )
    }

    fun testAnOverriddenMethodIsRefused() {
        assertRefused(
            """
            class A {
                int f() { return 1; }
            }
            class B extends A {
                @Override
                int f() { return 2; }
            }
            void main() { print(new A().f()); }
            """.trimIndent(),
            "f",
            "overridden",
        )
    }

    fun testTheHandlerIsRegistered() {
        assertTrue(InlineActionHandler.EP_NAME.extensionList.any { it is JuxInlineMethodHandler })
    }

    private fun doTest(before: String, methodName: String, after: String) {
        myFixture.configureByText("a.jux", before)
        JuxInlineMethodHandler().inlineElement(project, myFixture.editor, method(methodName))
        assertEquals(after, myFixture.editor.document.text)
    }

    private fun assertRefused(source: String, methodName: String, reasonPart: String) {
        myFixture.configureByText("a.jux", source)
        val before = myFixture.editor.document.text
        try {
            JuxInlineMethodHandler().inlineElement(project, myFixture.editor, method(methodName))
        } catch (e: CommonRefactoringUtil.RefactoringErrorHintException) {
            assertTrue("the reason should mention `$reasonPart`: ${e.message}", e.message!!.contains(reasonPart))
            assertEquals(before, myFixture.editor.document.text)
            return
        }
        throw AssertionError("expected a refusal, got:\n${myFixture.editor.document.text}")
    }

    private fun method(name: String) =
        PsiTreeUtil.findChildrenOfType(myFixture.file, JuxMethodDeclaration::class.java).first { it.name == name }
}
