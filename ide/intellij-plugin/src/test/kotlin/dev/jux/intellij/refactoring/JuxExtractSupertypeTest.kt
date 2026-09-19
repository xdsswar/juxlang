package dev.jux.intellij.refactoring

import com.intellij.psi.util.PsiTreeUtil
import com.intellij.refactoring.RefactoringActionHandler
import com.intellij.refactoring.util.CommonRefactoringUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxTypeDeclaration

/** Extract Interface and Extract Superclass. */
class JuxExtractSupertypeTest : BasePlatformTestCase() {

    override fun tearDown() {
        try {
            JuxRefactoringInput.answers.clear()
        } finally {
            super.tearDown()
        }
    }

    private fun run(handler: RefactoringActionHandler, typeName: String, source: String): String {
        myFixture.configureByText("a.jux", source.trimIndent())
        val type = PsiTreeUtil.findChildrenOfType(myFixture.file, JuxTypeDeclaration::class.java).first { it.name == typeName }
        handler.invoke(project, arrayOf(type), null)
        return myFixture.editor.document.text
    }

    private fun assertRefused(handler: RefactoringActionHandler, typeName: String, source: String, part: String) {
        myFixture.configureByText("a.jux", source.trimIndent())
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

    // ---- Extract Interface -----------------------------------------------------

    fun testExtractInterfaceWritesSignaturesAndImplements() {
        JuxRefactoringInput.answers["${JuxExtractInterfaceHandler.TITLE}: name"] = "Shape"
        JuxRefactoringInput.answers["${JuxExtractInterfaceHandler.TITLE}: members"] = "area, name"
        val after = run(
            JuxExtractInterfaceHandler(), "Circle",
            """
            public class Circle {
                private double r = 1.0;

                public double area() {
                    return 3.0 * r * r;
                }

                public String name() {
                    return "circle";
                }

                private void secret() {}
            }
            """,
        )
        assertTrue(after, after.contains("public class Circle implements Shape {"))
        assertTrue(after, after.contains("public interface Shape {"))
        assertTrue(after, after.contains("double area();"))
        assertTrue(after, after.contains("String name();"))
        assertFalse(after, after.contains("void secret();"))
        assertEquals(after, 2, Regex("@override").findAll(after).count())
    }

    fun testExtractInterfaceJoinsAnExistingImplementsList() {
        JuxRefactoringInput.answers["${JuxExtractInterfaceHandler.TITLE}: name"] = "Sized"
        JuxRefactoringInput.answers["${JuxExtractInterfaceHandler.TITLE}: members"] = "size"
        val after = run(
            JuxExtractInterfaceHandler(), "Bag",
            """
            interface Named { String name(); }
            class Bag implements Named {
                public String name() { return "bag"; }
                public int size() { return 0; }
            }
            """,
        )
        assertTrue(after, after.contains("class Bag implements Named, Sized {"))
        assertTrue(after, after.contains("int size();"))
    }

    fun testExtractInterfaceRefusesATakenName() {
        JuxRefactoringInput.answers["${JuxExtractInterfaceHandler.TITLE}: name"] = "Other"
        JuxRefactoringInput.answers["${JuxExtractInterfaceHandler.TITLE}: members"] = "run"
        assertRefused(
            JuxExtractInterfaceHandler(), "Task",
            """
            class Other {}
            class Task {
                public void run() {}
            }
            """,
            "already exists",
        )
    }

    // ---- Extract Superclass ----------------------------------------------------

    fun testExtractSuperclassMovesMembersAndExtends() {
        JuxRefactoringInput.answers["${JuxExtractSuperclassHandler.TITLE}: name"] = "Vehicle"
        JuxRefactoringInput.answers["${JuxExtractSuperclassHandler.TITLE}: members"] = "wheels, describe"
        val after = run(
            JuxExtractSuperclassHandler(), "Car",
            """
            class Car {
                private int wheels = 4;

                String describe() {
                    return ${'$'}"on ${'$'}{wheels}";
                }

                void honk() {
                    print("beep");
                }
            }
            """,
        )
        assertTrue(after, after.contains("class Car extends Vehicle {"))
        assertTrue(after, after.contains("class Vehicle {"))
        assertTrue(after, after.contains("protected int wheels = 4;"))
        val car = after.substringBefore("class Vehicle")
        assertFalse(after, car.contains("wheels = 4"))
        assertTrue(after, car.contains("void honk()"))
    }

    fun testExtractSuperclassAbstractMethodStaysBelow() {
        JuxRefactoringInput.answers["${JuxExtractSuperclassHandler.TITLE}: name"] = "Base"
        JuxRefactoringInput.answers["${JuxExtractSuperclassHandler.TITLE}: members"] = "area"
        JuxRefactoringInput.answers["${JuxExtractSuperclassHandler.TITLE}: abstract"] = "area"
        val after = run(
            JuxExtractSuperclassHandler(), "Square",
            """
            class Square {
                double side = 2.0;
                double area() { return side * side; }
            }
            """,
        )
        assertTrue(after, after.contains("abstract class Base {"))
        assertTrue(after, after.contains("abstract double area();"))
        assertTrue(after, after.contains("@override"))
        assertTrue(after, after.substringBefore("abstract class Base").contains("return side * side;"))
    }

    fun testExtractSuperclassTakesOverAnExistingExtends() {
        JuxRefactoringInput.answers["${JuxExtractSuperclassHandler.TITLE}: name"] = "Middle"
        JuxRefactoringInput.answers["${JuxExtractSuperclassHandler.TITLE}: members"] = "tag"
        val after = run(
            JuxExtractSuperclassHandler(), "Leaf",
            """
            class Root {}
            class Leaf extends Root {
                String tag() { return "leaf"; }
            }
            """,
        )
        assertTrue(after, after.contains("class Leaf extends Middle {"))
        assertTrue(after, after.contains("class Middle extends Root {"))
    }

    fun testExtractSuperclassRefusesWhenAMovedMethodNeedsWhatStays() {
        JuxRefactoringInput.answers["${JuxExtractSuperclassHandler.TITLE}: name"] = "Base"
        JuxRefactoringInput.answers["${JuxExtractSuperclassHandler.TITLE}: members"] = "describe"
        assertRefused(
            JuxExtractSuperclassHandler(), "Car",
            """
            class Car {
                private int wheels = 4;
                String describe() { return ${'$'}"${'$'}{wheels}" + wheels; }
            }
            """,
            "wheels",
        )
    }
}
