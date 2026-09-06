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

    fun testAnUnknownTypeIsRefusedRatherThanGuessed() {
        // A guessed type in a field declaration compiles into a different
        // program instead of surfacing as a question, so nothing is written.
        val source = """
            class C {
                void m() { print(<selection>whoKnows()</selection>); }
            }
        """.trimIndent()
        myFixture.configureByText("a.jux", source)
        val before = myFixture.editor.document.text
        try {
            JuxIntroduceConstantHandler().invoke(project, myFixture.editor, myFixture.file, null)
            throw AssertionError("expected the refactoring to be refused, but it ran")
        } catch (e: CommonRefactoringUtil.RefactoringErrorHintException) {
            assertTrue("the message should say what is missing: ${e.message}", e.message!!.contains("type"))
            assertEquals("a refused refactoring must not touch the file", before, myFixture.editor.document.text)
        }
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
