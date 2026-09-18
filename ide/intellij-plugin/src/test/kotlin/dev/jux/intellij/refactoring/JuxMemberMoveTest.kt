package dev.jux.intellij.refactoring

import com.intellij.psi.util.PsiTreeUtil
import com.intellij.refactoring.util.CommonRefactoringUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxTypeDeclaration

/** Pull Members Up and Push Members Down. */
class JuxMemberMoveTest : BasePlatformTestCase() {

    override fun tearDown() {
        try {
            JuxRefactoringInput.answers.clear()
        } finally {
            super.tearDown()
        }
    }

    fun testPullUpMovesAFieldAndAMethodTogether() {
        JuxRefactoringInput.answers[JuxPullUpHandler.TITLE] = "wheels, describe"
        val after = run(
            JuxPullUpHandler(), "Car",
            """
            class Vehicle {
                protected String name = "v";
            }

            class Car extends Vehicle {
                private int wheels = 4;

                String describe() {
                    return ${'$'}"${'$'}{name} on ${'$'}{wheels}";
                }

                void honk() {
                    print("beep");
                }
            }
            """.trimIndent(),
        )
        assertEquals(
            """
            class Vehicle {
                protected String name = "v";
                protected int wheels = 4;

                String describe() {
                    return ${'$'}"${'$'}{name} on ${'$'}{wheels}";
                }
            }

            class Car extends Vehicle {
                void honk() {
                    print("beep");
                }
            }
            """.trimIndent(),
            after,
        )
    }

    fun testPullUpAsAbstractLeavesTheBodyBelow() {
        JuxRefactoringInput.answers[JuxPullUpHandler.TITLE] = "area"
        JuxRefactoringInput.answers["${JuxPullUpHandler.TITLE}: abstract"] = "area"
        val after = run(
            JuxPullUpHandler(), "Square",
            """
            class Shape {
            }

            class Square extends Shape {
                double side = 1.0;

                double area() {
                    return side * side;
                }
            }
            """.trimIndent(),
        )
        assertTrue(after, after.contains("abstract class Shape {"))
        assertTrue(after, after.contains("abstract double area();"))
        assertTrue(after, after.contains("@Override\n    double area() {"))
    }

    fun testPullingUpAMethodWithoutTheFieldItReadsIsRefused() {
        JuxRefactoringInput.answers[JuxPullUpHandler.TITLE] = "describe"
        assertRefused(
            JuxPullUpHandler(), "Car",
            """
            class Vehicle {}

            class Car extends Vehicle {
                private int wheels = 4;

                String describe() {
                    return ${'$'}"${'$'}{wheels}";
                }
            }
            """.trimIndent(),
            "`wheels`",
        )
    }

    fun testPushDownCopiesToEverySubclass() {
        JuxRefactoringInput.answers[JuxPushDownHandler.TITLE] = "greet"
        val after = run(
            JuxPushDownHandler(), "Animal",
            """
            class Animal {
                void greet() {
                    print("hi");
                }
            }

            class Dog extends Animal {
            }

            class Cat extends Animal {
                void purr() {}
            }
            """.trimIndent(),
        )
        assertEquals(
            """
            class Animal {
            }

            class Dog extends Animal {
                void greet() {
                    print("hi");
                }
            }

            class Cat extends Animal {
                void purr() {}

                void greet() {
                    print("hi");
                }
            }
            """.trimIndent(),
            after,
        )
    }

    fun testPushingDownWhatTheSuperclassStillUsesIsRefused() {
        JuxRefactoringInput.answers[JuxPushDownHandler.TITLE] = "count"
        assertRefused(
            JuxPushDownHandler(), "Base",
            """
            class Base {
                int count = 0;

                void bump() {
                    count = count + 1;
                }
            }

            class Leaf extends Base {}
            """.trimIndent(),
            "`count`",
        )
    }

    fun testTheHandlersAreRegistered() {
        val provider = com.intellij.lang.LanguageRefactoringSupport.getInstance().forLanguage(dev.jux.intellij.JuxLanguage)
        assertTrue(provider.pullUpHandler is JuxPullUpHandler)
        assertTrue(provider.pushDownHandler is JuxPushDownHandler)
    }

    private fun run(handler: com.intellij.refactoring.RefactoringActionHandler, typeName: String, source: String): String {
        myFixture.configureByText("a.jux", source)
        val type = PsiTreeUtil.findChildrenOfType(myFixture.file, JuxTypeDeclaration::class.java).first { it.name == typeName }
        handler.invoke(project, arrayOf(type), null)
        return myFixture.editor.document.text
    }

    private fun assertRefused(handler: com.intellij.refactoring.RefactoringActionHandler, typeName: String, source: String, part: String) {
        myFixture.configureByText("a.jux", source)
        val before = myFixture.editor.document.text
        val type = PsiTreeUtil.findChildrenOfType(myFixture.file, JuxTypeDeclaration::class.java).first { it.name == typeName }
        try {
            handler.invoke(project, arrayOf(type), null)
        } catch (e: CommonRefactoringUtil.RefactoringErrorHintException) {
            assertTrue(e.message, e.message!!.contains(part))
            assertEquals(before, myFixture.editor.document.text)
            return
        }
        fail("expected a refusal, got:\n${myFixture.editor.document.text}")
    }
}
