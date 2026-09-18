package dev.jux.intellij.refactoring

import com.intellij.refactoring.util.CommonRefactoringUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Extract Method (`Ctrl+Alt+M`): Java's data-flow rules on Jux code. Inputs
 * become parameters, one output becomes the return value, and anything that
 * would change the program is refused with the reason.
 */
class JuxExtractMethodTest : BasePlatformTestCase() {

    override fun tearDown() {
        try {
            JuxRefactoringInput.answers.clear()
        } finally {
            super.tearDown()
        }
    }

    fun testStatementsReadingParametersBecomeAVoidMethod() {
        doTest(
            """
            class Shop {
                void sell(String item, int qty) {
                    <selection>print(item);
                    print(qty);</selection>
                }
            }
            """.trimIndent(),
            """
            class Shop {
                void sell(String item, int qty) {
                    extracted(item, qty);
                }

                private void extracted(String item, int qty) {
                    print(item);
                    print(qty);
                }
            }
            """.trimIndent(),
        )
    }

    fun testALocalReadAfterwardsBecomesTheReturnValue() {
        doTest(
            """
            class Calc {
                int total(int a, int b) {
                    <selection>int sum = a + b;
                    sum = sum * 2;</selection>
                    return sum;
                }
            }
            """.trimIndent(),
            """
            class Calc {
                int total(int a, int b) {
                    int sum = extracted(a, b);
                    return sum;
                }

                private int extracted(int a, int b) {
                    int sum = a + b;
                    sum = sum * 2;
                    return sum;
                }
            }
            """.trimIndent(),
        )
    }

    fun testAnExpressionBecomesAReturningMethod() {
        doTest(
            """
            class Calc {
                int area(int w, int h) {
                    return <selection>w * h</selection> + 1;
                }
            }
            """.trimIndent(),
            """
            class Calc {
                int area(int w, int h) {
                    return extracted(w, h) + 1;
                }

                private int extracted(int w, int h) {
                    return w * h;
                }
            }
            """.trimIndent(),
        )
    }

    fun testStaticContextGivesAStaticMethodAndTheNameIsAsked() {
        JuxRefactoringInput.answers[JuxExtractMethodHandler.TITLE] = "greet"
        doTest(
            """
            class App {
                static void main() {
                    var name = "ann";
                    <selection>print(${'$'}"hi ${'$'}{name}");</selection>
                }
            }
            """.trimIndent(),
            """
            class App {
                static void main() {
                    var name = "ann";
                    greet(name);
                }

                private static void greet(String name) {
                    print(${'$'}"hi ${'$'}{name}");
                }
            }
            """.trimIndent(),
        )
    }

    fun testATopLevelFunctionGetsATopLevelFunction() {
        doTest(
            """
            void main() {
                int n = 3;
                <selection>print(n * n);</selection>
            }
            """.trimIndent(),
            """
            void main() {
                int n = 3;
                extracted(n);
            }

            void extracted(int n) {
                print(n * n);
            }
            """.trimIndent(),
        )
    }

    fun testTwoOutputsAreRefused() {
        assertRefused(
            """
            class C {
                void m() {
                    <selection>int a = 1;
                    int b = 2;</selection>
                    print(a + b);
                }
            }
            """.trimIndent(),
            "only one value",
        )
    }

    fun testAReturnIsRefused() {
        assertRefused(
            """
            class C {
                int m(int x) {
                    <selection>if (x > 0) {
                        return 1;
                    }</selection>
                    return 0;
                }
            }
            """.trimIndent(),
            "return",
        )
    }

    fun testABreakLeavingTheSelectionIsRefused() {
        assertRefused(
            """
            class C {
                void m() {
                    while (true) {
                        <selection>print(1);
                        break;</selection>
                    }
                }
            }
            """.trimIndent(),
            "break",
        )
    }

    fun testAPartialStatementIsRefused() {
        assertRefused(
            """
            class C {
                void m() {
                    <selection>print(1);
                    pri</selection>nt(2);
                }
            }
            """.trimIndent(),
            "whole",
        )
    }

    private fun doTest(before: String, after: String) {
        myFixture.configureByText("a.jux", before)
        JuxExtractMethodHandler().invoke(project, myFixture.editor, myFixture.file, null)
        assertEquals(after, myFixture.editor.document.text)
    }

    private fun assertRefused(source: String, reasonPart: String) {
        myFixture.configureByText("a.jux", source)
        val before = myFixture.editor.document.text
        try {
            JuxExtractMethodHandler().invoke(project, myFixture.editor, myFixture.file, null)
        } catch (e: CommonRefactoringUtil.RefactoringErrorHintException) {
            assertTrue("the reason should mention `$reasonPart`: ${e.message}", e.message!!.contains(reasonPart))
            assertEquals("a refused refactoring must not touch the file", before, myFixture.editor.document.text)
            return
        }
        throw AssertionError("expected the refactoring to be refused, but it ran:\n${myFixture.editor.document.text}")
    }
}
