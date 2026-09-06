package dev.jux.intellij.editor

import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * [JuxBreadcrumbsProvider] — the trail under the editor.
 *
 * The plugin's marketplace description has claimed breadcrumbs since the first
 * release; until this provider existed the strip was empty. These tests are the
 * claim, written down.
 */
class JuxBreadcrumbsTest : BasePlatformTestCase() {

    private val provider = JuxBreadcrumbsProvider()

    fun testTrailWalksOuterTypeThenNestedTypeThenMethod() {
        val trail = trailAtCaret(
            """
            class Parser {
                class Cursor {
                    void advance() { var x = <caret>1; }
                }
            }
            """.trimIndent(),
        )
        assertEquals(listOf("Parser", "Cursor", "advance()"), trail)
    }

    fun testAMethodCrumbCarriesItsParentheses() {
        // `advance` alone could be a field; the parentheses say which it is.
        val trail = trailAtCaret("class C { void advance() { var x = <caret>1; } }")
        assertEquals(listOf("C", "advance()"), trail)
    }

    fun testStatementsDoNotEarnACrumb() {
        // A trail of `if > for > if` would push the enclosing class off the
        // left edge, which is the one thing the trail exists to show.
        val trail = trailAtCaret(
            """
            class C {
                void m() {
                    if (flag) {
                        for (var i : xs) { var y = <caret>1; }
                    }
                }
            }
            """.trimIndent(),
        )
        assertEquals(listOf("C", "m()"), trail)
    }

    fun testAPropertyEarnsACrumb() {
        val trail = trailAtCaret(
            """
            class C {
                public String Title {
                    get { return <caret>"x"; }
                }
            }
            """.trimIndent(),
        )
        assertEquals(listOf("C", "Title"), trail)
    }

    fun testTheTooltipNamesTheKind() {
        myFixture.configureByText("a.jux", "interface Drawable { void draw(); }")
        val type = myFixture.file.children.first { provider.acceptElement(it) }
        assertEquals("interface Drawable", provider.getElementTooltip(type))
    }

    /** The accepted ancestors of the caret, outermost first. */
    private fun trailAtCaret(source: String): List<String> {
        myFixture.configureByText("a.jux", source)
        val out = ArrayList<String>()
        var e: PsiElement? = myFixture.file.findElementAt(myFixture.caretOffset)
        while (e != null && e !is PsiFile) {
            if (provider.acceptElement(e)) out.add(provider.getElementInfo(e))
            e = e.parent
        }
        return out.reversed()
    }
}
