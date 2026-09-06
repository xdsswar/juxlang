package dev.jux.intellij.refactoring

import com.intellij.codeInsight.template.impl.TemplateManagerImpl
import com.intellij.refactoring.util.CommonRefactoringUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Introduce Constant (`Ctrl+Alt+C`).
 *
 * The field form is the corpus idiom, `private static final <T> NAME = …;`. A
 * field needs a written type, so the interesting cases are the ones where the
 * type can and cannot be read off the source.
 */
class JuxIntroduceConstantTest : BasePlatformTestCase() {

    fun testExtractsAStringLiteral() {
        val after = extract(
            """
            class C {
                void m() { print(<selection>"hello"</selection>); }
            }
            """.trimIndent(),
        )
        assertTrue("expected a String constant, got:\n$after", after.contains("private static final String CONSTANT = \"hello\";"))
        assertTrue("expected the use to be replaced, got:\n$after", after.contains("print(CONSTANT);"))
    }

    fun testInfersTheNumericTypeFromTheLiteral() {
        assertTrue(typeOfExtracted("42").contains("int CONSTANT = 42;"))
        assertTrue(typeOfExtracted("3.14").contains("double CONSTANT = 3.14;"))
        assertTrue(typeOfExtracted("true").contains("bool CONSTANT = true;"))
        assertTrue(typeOfExtracted("'x'").contains("char CONSTANT = 'x';"))
    }

    fun testTakesTheTypeFromANewExpression() {
        val after = extract(
            """
            class Point { }
            class C {
                void m() { var p = <selection>new Point()</selection>; }
            }
            """.trimIndent(),
        )
        assertTrue("expected the constructed type, got:\n$after", after.contains("private static final Point CONSTANT = new Point();"))
    }

    fun testTakesTheReturnTypeOfACallInTheEnclosingType() {
        val after = extract(
            """
            class C {
                String label() { return "x"; }
                void m() { print(<selection>label()</selection>); }
            }
            """.trimIndent(),
        )
        assertTrue("expected the return type, got:\n$after", after.contains("private static final String CONSTANT = label();"))
    }

    fun testTakesTheDeclaredTypeOfAFieldReference() {
        // Generic arguments must survive whole: a field declaration has to
        // spell the type out, and `Map` alone would not compile.
        val after = extract(
            """
            class C {
                Map<String, Leaf> table;
                void m() { print(<selection>table</selection>); }
            }
            """.trimIndent(),
        )
        assertTrue("expected the full generic type, got:\n$after", after.contains("private static final Map<String, Leaf> CONSTANT = table;"))
    }

    fun testTakesTheTypeOfAMemberThroughAReceiver() {
        val after = extract(
            """
            class Engine { int rpm; }
            class Car {
                Engine engine;
                void m() { print(<selection>engine.rpm</selection>); }
            }
            """.trimIndent(),
        )
        assertTrue("expected the member's type, got:\n$after", after.contains("private static final int CONSTANT = engine.rpm;"))
    }

    fun testACastStatesItsOwnType() {
        val after = extract(
            """
            class Shape { }
            class C {
                void m(Object o) { print(<selection>(Shape) o</selection>); }
            }
            """.trimIndent(),
        )
        assertTrue("expected the cast type, got:\n$after", after.contains("private static final Shape CONSTANT = (Shape) o;"))
    }

    fun testAVoidCallIsNotAConstant() {
        // `void` is a return type, not a type a value can have.
        assertRefused(
            """
            class C {
                void work() { }
                void m() { <selection>work()</selection>; }
            }
            """.trimIndent(),
        )
    }

    fun testAnUnknownTypeIsRefusedRatherThanGuessed() {
        // Arithmetic needs the type checker, and a guessed type in a field
        // declaration compiles into a different program instead of surfacing
        // as a question -- so nothing is written.
        assertRefused(
            """
            class C {
                void m(int a, int b) { print(<selection>a + b</selection>); }
            }
            """.trimIndent(),
        )
    }

    /** Assert the handler declines the source and leaves the file untouched. */
    private fun assertRefused(source: String) {
        myFixture.configureByText("a.jux", source)
        val before = myFixture.editor.document.text
        try {
            JuxIntroduceConstantHandler().invoke(project, myFixture.editor, myFixture.file, null)
        } catch (e: CommonRefactoringUtil.RefactoringErrorHintException) {
            assertTrue("the message should say what is missing: ${e.message}", e.message!!.contains("type"))
            assertEquals("a refused refactoring must not touch the file", before, myFixture.editor.document.text)
            return
        }
        throw AssertionError("expected the refactoring to be refused, but it ran")
    }

    fun testTheConstantIsGroupedWithTheExistingFields() {
        val after = extract(
            """
            class C {
                private int count = 0;
                void m() { print(<selection>"hi"</selection>); }
            }
            """.trimIndent(),
        )
        assertTrue(
            "the new field should follow the existing one:\n$after",
            after.indexOf("private int count = 0;") < after.indexOf("private static final String CONSTANT"),
        )
        assertTrue(
            "the new field should precede the method:\n$after",
            after.indexOf("private static final String CONSTANT") < after.indexOf("void m()"),
        )
    }

    private fun typeOfExtracted(literal: String): String = extract(
        """
        class C {
            void m() { var x = <selection>$literal</selection>; }
        }
        """.trimIndent(),
    )

    private fun extract(source: String): String {
        myFixture.configureByText("a.jux", source)
        TemplateManagerImpl.setTemplateTesting(testRootDisposable)
        JuxIntroduceConstantHandler().invoke(project, myFixture.editor, myFixture.file, null)
        TemplateManagerImpl.getTemplateState(myFixture.editor)?.gotoEnd(false)
        return myFixture.editor.document.text
    }
}
