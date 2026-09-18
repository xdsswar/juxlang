package dev.jux.intellij.refactoring

import com.intellij.refactoring.util.CommonRefactoringUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/** Introduce Field (`Ctrl+Alt+F`) and Introduce Parameter (`Ctrl+Alt+P`). */
class JuxIntroduceMemberTest : BasePlatformTestCase() {

    override fun tearDown() {
        try {
            JuxRefactoringInput.answers.clear()
        } finally {
            super.tearDown()
        }
    }

    fun testAFieldInitializedInItsDeclaration() {
        myFixture.configureByText(
            "a.jux",
            """
            class Shop {
                private int stock = 3;

                void report() {
                    print(<selection>new Vec<String>()</selection>);
                }
            }
            """.trimIndent(),
        )
        JuxIntroduceFieldHandler().invoke(project, myFixture.editor, myFixture.file, null)
        assertEquals(
            """
            class Shop {
                private int stock = 3;
                private Vec<String> vec = new Vec<String>();

                void report() {
                    print(vec);
                }
            }
            """.trimIndent(),
            myFixture.editor.document.text,
        )
    }

    fun testAFieldReadingAParameterIsAssignedInTheMethod() {
        JuxRefactoringInput.answers[JuxIntroduceFieldHandler.TITLE] = "doubled"
        myFixture.configureByText(
            "a.jux",
            """
            class Calc {
                static int twice(int n) {
                    print(<selection>n * 2</selection>);
                    return n * 2;
                }
            }
            """.trimIndent(),
        )
        JuxIntroduceFieldHandler().invoke(project, myFixture.editor, myFixture.file, null)
        assertEquals(
            """
            class Calc {
                private static int doubled;

                static int twice(int n) {
                    doubled = n * 2;
                    print(doubled);
                    return doubled;
                }
            }
            """.trimIndent(),
            myFixture.editor.document.text,
        )
    }

    fun testAParameterIsPassedAtEveryCallInTheCallersTerms() {
        val shop = myFixture.addFileToProject(
            "Shop.jux",
            """
            public class Shop {
                public static int price(int qty) {
                    return qty * 3 + 1;
                }
            }
            """.trimIndent(),
        )
        val app = myFixture.addFileToProject(
            "App.jux",
            """
            void main() {
                int n = 2;
                print(Shop.price(n + 1));
            }
            """.trimIndent(),
        )
        myFixture.configureFromExistingVirtualFile(shop.virtualFile)
        val start = shop.text.indexOf("qty * 3")
        myFixture.editor.selectionModel.setSelection(start, start + "qty * 3".length)
        JuxRefactoringInput.answers[JuxIntroduceParameterHandler.TITLE] = "base"
        JuxIntroduceParameterHandler().invoke(project, myFixture.editor, myFixture.file, null)
        assertEquals(
            """
            public class Shop {
                public static int price(int qty, int base) {
                    return base + 1;
                }
            }
            """.trimIndent(),
            shop.text,
        )
        assertTrue(app.text, app.text.contains("print(Shop.price(n + 1, (n + 1) * 3));"))
    }

    fun testAParameterReadingALocalIsRefused() {
        myFixture.configureByText(
            "a.jux",
            """
            int f(int a) {
                int b = a + 1;
                return <selection>b * 2</selection>;
            }
            """.trimIndent(),
        )
        try {
            JuxIntroduceParameterHandler().invoke(project, myFixture.editor, myFixture.file, null)
        } catch (e: CommonRefactoringUtil.RefactoringErrorHintException) {
            assertTrue(e.message, e.message!!.contains("local `b`"))
            return
        }
        fail("expected a refusal")
    }

    fun testNamesComeFromTheExpression() {
        myFixture.configureByText("a.jux", "void f() { var x = getTotal(); var y = order.count; var z = new Basket(); }")
        val file = myFixture.file
        fun at(text: String) = com.intellij.psi.util.PsiTreeUtil.findChildrenOfAnyType(
            file, dev.jux.intellij.psi.JuxCompositeElement::class.java,
        ).first { it.text == text }
        assertEquals("total", JuxNameSuggester.suggest(at("getTotal()")))
        assertEquals("count", JuxNameSuggester.suggest(at("order.count")))
        assertEquals("basket", JuxNameSuggester.suggest(at("new Basket()")))
    }
}
