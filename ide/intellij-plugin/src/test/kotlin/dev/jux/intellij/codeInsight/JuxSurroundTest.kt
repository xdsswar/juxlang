package dev.jux.intellij.codeInsight

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.codeInsight.surround.JuxExpressionSurroundDescriptor
import dev.jux.intellij.codeInsight.surround.JuxStatementSurroundDescriptor

/**
 * Surround With (`Ctrl+Alt+T`) — the descriptors decide what may be surrounded,
 * and the surrounders write the text.
 *
 * The applicability half is tested as carefully as the rewriting half: a
 * template offered over the wrong selection produces source that no longer
 * parses, which is worse than the template not being offered at all.
 */
class JuxSurroundTest : BasePlatformTestCase() {

    private val statements = JuxStatementSurroundDescriptor()
    private val expressions = JuxExpressionSurroundDescriptor()

    fun testTheStatementTemplatesAreTheJavaSet() {
        val offered = statements.surrounders.map { it.templateDescription }
        assertContainsElements(
            offered,
            "if", "if / else", "while", "do / while", "for",
            "try / catch", "try / finally", "try / catch / finally",
            "{ } block", "unsafe { }",
        )
    }

    fun testASelectedStatementIsSurroundable() {
        myFixture.configureByText(
            "a.jux",
            """
            class C {
                void m() {
                    <selection>work();</selection>
                }
            }
            """.trimIndent(),
        )
        val elements = elementsToSurround(statements)
        assertEquals(1, elements.size)
        assertTrue(statements.surrounders.first().isApplicable(elements))
    }

    fun testTwoConsecutiveStatementsSurroundTogether() {
        myFixture.configureByText(
            "a.jux",
            """
            class C {
                void m() {
                    <selection>var a = 1;
                    var b = 2;</selection>
                }
            }
            """.trimIndent(),
        )
        assertEquals(2, elementsToSurround(statements).size)
    }

    fun testHalfAStatementIsNotSurroundable() {
        // A selection that clips a statement must offer nothing: wrapping it
        // would emit `if (true) { var a = }`.
        myFixture.configureByText(
            "a.jux",
            """
            class C {
                void m() {
                    <selection>var a = </selection>1;
                }
            }
            """.trimIndent(),
        )
        assertEmpty(elementsToSurround(statements))
    }

    fun testAnExpressionIsSurroundableAndAStatementIsNot() {
        myFixture.configureByText(
            "a.jux",
            """
            class C {
                void m() { var a = <selection>b + c</selection>; }
            }
            """.trimIndent(),
        )
        val elements = elementsToSurround(expressions)
        assertEquals(1, elements.size)
        assertTrue(expressions.surrounders.first().isApplicable(elements))
        // The statement templates must decline the same selection.
        assertFalse(statements.surrounders.first().isApplicable(elements))
    }

    fun testTheExpressionTemplatesAreParensAndNegation() {
        assertEquals(
            listOf("(expr)", "!(expr)"),
            expressions.surrounders.map { it.templateDescription },
        )
    }

    private fun elementsToSurround(
        descriptor: com.intellij.lang.surroundWith.SurroundDescriptor,
    ): Array<com.intellij.psi.PsiElement> {
        val selection = myFixture.editor.selectionModel
        return descriptor.getElementsToSurround(
            myFixture.file,
            selection.selectionStart,
            selection.selectionEnd,
        )
    }
}
