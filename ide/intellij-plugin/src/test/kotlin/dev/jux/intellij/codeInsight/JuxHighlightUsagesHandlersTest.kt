package dev.jux.intellij.codeInsight

import com.intellij.codeInsight.highlighting.HighlightUsagesHandler
import com.intellij.codeInsight.highlighting.HighlightUsagesHandlerBase
import com.intellij.psi.PsiElement
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Keyword highlight-usages handlers: what gets highlighted with the caret on
 * `return`, `break`, `try`, `catch`, `throws` and `extends`, as in Java.
 */
class JuxHighlightUsagesHandlersTest : BasePlatformTestCase() {

    /** The texts highlighted for the keyword at the caret. */
    private fun highlighted(text: String): List<String> {
        myFixture.configureByText("a.jux", text)
        @Suppress("UNCHECKED_CAST")
        val handler = HighlightUsagesHandler.createCustomHandler<PsiElement>(myFixture.editor, myFixture.file)
            as HighlightUsagesHandlerBase<PsiElement>?
        assertNotNull("no handler at the caret", handler)
        handler!!.computeUsages(handler.targets)
        val doc = myFixture.editor.document
        return (handler.readUsages + handler.writeUsages).map { doc.getText(it) }
    }

    fun testReturnHighlightsEveryExitPoint() {
        val found = highlighted(
            """
            int f(int x) {
                if (x < 0) { throw new IllegalArgumentException("neg"); }
                if (x == 0) { re<caret>turn 0; }
                var g = () -> { return 9; };
                try { throw new IOException("io"); } catch (IOException e) { }
                return x;
            }
            """.trimIndent(),
        )
        assertSameElements(
            found,
            listOf("throw new IllegalArgumentException(\"neg\");", "return 0;", "return x;"),
        )
    }

    fun testBreakHighlightsTheLoopItLeaves() {
        val found = highlighted(
            """
            void f(int[] xs) {
                outer: while (true) {
                    for (var x : xs) {
                        if (x == 1) br<caret>eak outer;
                        if (x == 2) continue;
                    }
                    break;
                }
            }
            """.trimIndent(),
        )
        // The labeled `while`, the other `break` leaving it, and the keyword.
        assertSameElements(found, listOf("while", "break", "break"))
    }

    fun testContinueHighlightsItsOwnLoop() {
        val found = highlighted(
            """
            void f(int[] xs) {
                for (var x : xs) {
                    if (x == 1) cont<caret>inue;
                    if (x == 2) break;
                }
            }
            """.trimIndent(),
        )
        assertSameElements(found, listOf("for", "continue", "break"))
    }

    fun testCatchHighlightsWhatItCatches() {
        val found = highlighted(
            """
            void f(bool a) {
                try {
                    if (a) { throw new IOException("io"); }
                    throw new IllegalStateException("state");
                } cat<caret>ch (IOException e) {
                } catch (Exception e) {
                }
            }
            """.trimIndent(),
        )
        assertSameElements(found, listOf("throw new IOException(\"io\");", "catch"))
    }

    fun testTryHighlightsEverythingItsBodyThrows() {
        val found = highlighted(
            """
            void risky() throws IOException { }
            void f() {
                t<caret>ry {
                    risky();
                    throw new IllegalStateException("state");
                } catch (Exception e) {
                }
            }
            """.trimIndent(),
        )
        assertSameElements(found, listOf("risky()", "throw new IllegalStateException(\"state\");", "try"))
    }

    fun testExtendsHighlightsTheOverridingMethods() {
        val found = highlighted(
            """
            class Base { public void a() { } public void b() { } }
            class Child ext<caret>ends Base {
                public void a() { }
                public void own() { }
            }
            """.trimIndent(),
        )
        assertSameElements(found, listOf("a", "extends"))
    }
}
