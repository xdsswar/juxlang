package dev.jux.intellij.editor

import com.intellij.openapi.actionSystem.IdeActions
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * [JuxStatementMover] — Move Statement Up / Down.
 *
 * The two properties worth pinning: a multi-line construct moves whole, and a
 * statement at the edge of its block does not escape it.
 */
class JuxStatementMoverTest : BasePlatformTestCase() {

    fun testMovesAStatementDownPastItsNeighbour() {
        val after = move(
            """
            class C {
                void m() {
                    var a = 1;<caret>
                    var b = 2;
                }
            }
            """.trimIndent(),
            down = true,
        )
        assertTrue("expected `b` to come first now, got:\n$after", after.indexOf("var b = 2;") < after.indexOf("var a = 1;"))
    }

    fun testMovesAStatementUpPastItsNeighbour() {
        val after = move(
            """
            class C {
                void m() {
                    var a = 1;
                    var b = 2;<caret>
                }
            }
            """.trimIndent(),
            down = false,
        )
        assertTrue("expected `b` to come first now, got:\n$after", after.indexOf("var b = 2;") < after.indexOf("var a = 1;"))
    }

    fun testAMultiLineStatementMovesWhole() {
        val after = move(
            """
            class C {
                void m() {
                    if (flag) {
                        work();
                    }<caret>
                    var tail = 1;
                }
            }
            """.trimIndent(),
            down = true,
        )
        // The header must still sit above its own body: a line-wise move would
        // have left `if (flag) {` behind.
        assertTrue("the `if` was torn apart:\n$after", after.indexOf("if (flag) {") < after.indexOf("work();"))
        assertTrue("expected the `if` to move below the tail:\n$after", after.indexOf("var tail = 1;") < after.indexOf("if (flag) {"))
    }

    fun testTheLastStatementDoesNotLeaveItsBlock() {
        val source = """
            class C {
                void m() {
                    var only = 1;<caret>
                }
                void other() { }
            }
        """.trimIndent()
        val after = move(source, down = true)
        assertEquals("a lone statement must not escape its method", source.replace("<caret>", ""), after)
    }

    fun testMembersReorderInsideAClassBody() {
        val after = move(
            """
            class C {
                void first() { }<caret>
                void second() { }
            }
            """.trimIndent(),
            down = true,
        )
        assertTrue("expected the members to swap:\n$after", after.indexOf("void second()") < after.indexOf("void first()"))
    }

    private fun move(source: String, down: Boolean): String {
        myFixture.configureByText("a.jux", source)
        myFixture.performEditorAction(
            if (down) IdeActions.ACTION_MOVE_STATEMENT_DOWN_ACTION
            else IdeActions.ACTION_MOVE_STATEMENT_UP_ACTION,
        )
        return myFixture.editor.document.text
    }
}
