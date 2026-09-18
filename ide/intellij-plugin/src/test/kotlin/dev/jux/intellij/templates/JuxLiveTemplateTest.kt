package dev.jux.intellij.templates

import com.intellij.codeInsight.template.TemplateActionContext
import com.intellij.codeInsight.template.impl.TemplateManagerImpl
import com.intellij.openapi.actionSystem.IdeActions
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Live templates, Java's way: each is offered only where it makes sense
 * (statement, expression, declaration), and the macro-driven ones (`iter`,
 * `itar`, `soutv`, `soutm`, `soutp`, `psvm`) read the code around them.
 */
class JuxLiveTemplateTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        TemplateManagerImpl.setTemplateTesting(testRootDisposable)
    }

    /** The template names offered at the caret. */
    private fun offered(text: String): Set<String> {
        myFixture.configureByText("a.jux", text)
        // As the platform does before expanding: judge the context with an
        // identifier typed at the caret.
        val context = TemplateActionContext.expanding(myFixture.file, myFixture.editor)
        return TemplateManagerImpl.listApplicableTemplateWithInsertingDummyIdentifier(context).map { it.key }.toSet()
    }

    /** Type [abbreviation] at the caret, expand it, and accept every default. */
    private fun expand(text: String, abbreviation: String): String {
        myFixture.configureByText("a.jux", text)
        myFixture.type(abbreviation)
        myFixture.performEditorAction(IdeActions.ACTION_EXPAND_LIVE_TEMPLATE_BY_TAB)
        WriteCommandAction.runWriteCommandAction(project) {
            TemplateManagerImpl.getTemplateState(myFixture.editor)?.gotoEnd(false)
        }
        return myFixture.editor.document.text
    }

    fun testStatementTemplatesOnlyInBodies() {
        val inBody = offered("void main() {\n    <caret>\n}\n")
        assertTrue(inBody.toString(), "sout" in inBody)
        assertTrue(inBody.toString(), "iter" in inBody)
        assertFalse("psvm is a declaration: $inBody", "psvm" in inBody)
        assertFalse("prop is a member: $inBody", "prop" in inBody)

        val inType = offered("public class A {\n    <caret>\n}\n")
        assertTrue(inType.toString(), "psvm" in inType)
        assertTrue(inType.toString(), "prop" in inType)
        assertFalse("sout is a statement: $inType", "sout" in inType)
    }

    fun testNothingOfferedInCommentsOrStrings() {
        assertTrue(offered("void main() {\n    // <caret>\n}\n").isEmpty())
        assertTrue(offered("void main() {\n    var s = \"<caret>\";\n}\n").isEmpty())
    }

    fun testPsvmMatchesWhereItIsWritten() {
        val top = expand("<caret>\n", "psvm")
        assertTrue(top, top.contains("void main() {"))
        assertFalse(top, top.contains("static"))

        val inType = expand("public class App {\n    <caret>\n}\n", "psvm")
        assertTrue(inType, inType.contains("public static void main() {"))
    }

    fun testIterPicksTheNearestIterableAndNamesItsElement() {
        val text = expand(
            """
            void main() {
                int count = 3;
                var names = new Vec<String>();
                <caret>
            }
            """.trimIndent(),
            "iter",
        )
        assertTrue(text, text.contains("for (var name : names) {"))
    }

    fun testItarWalksAnArrayByIndex() {
        val text = expand(
            """
            void main() {
                int[] scores = [1, 2, 3];
                <caret>
            }
            """.trimIndent(),
            "itar",
        )
        assertTrue(text, text.contains("for (var i = 0; i < scores.length; i++) {"))
        assertTrue(text, text.contains("var score = scores[i];"))
    }

    fun testSoutvPrintsTheNearestVariableWithItsName() {
        val text = expand(
            """
            void main() {
                var total = 42;
                <caret>
            }
            """.trimIndent(),
            "soutv",
        )
        assertTrue(text, text.contains("print(\$\"total = \${total}\");"))
    }

    fun testSoutmAndSoutpDescribeTheMethod() {
        val m = expand(
            """
            public class Shop {
                public void sell(String item, int qty) {
                    <caret>
                }
            }
            """.trimIndent(),
            "soutm",
        )
        assertTrue(m, m.contains("print(\"Shop.sell\");"))

        val p = expand(
            """
            public class Shop {
                public void sell(String item, int qty) {
                    <caret>
                }
            }
            """.trimIndent(),
            "soutp",
        )
        assertTrue(p, p.contains("print(\$\"item = \${item}, qty = \${qty}\");"))
    }

    fun testElementNames() {
        val name = JuxElementNameMacro::elementName
        assertEquals("item", name("items"))
        assertEquals("entry", name("entries"))
        assertEquals("box", name("boxes"))
        assertEquals("name", name("nameList"))
        assertEquals("item", name("data"))
        assertEquals("item", name(""))
    }
}
