package dev.jux.intellij.refactoring

import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxMethodDeclaration

/** Introduce Parameter Object: chosen parameters become a record. */
class JuxIntroduceParameterObjectTest : BasePlatformTestCase() {

    override fun tearDown() {
        try {
            JuxRefactoringInput.answers.clear()
        } finally {
            super.tearDown()
        }
    }

    fun testChosenParametersBecomeARecordEverywhere() {
        val shop = myFixture.addFileToProject(
            "shop/Shop.jux",
            """
            package shop;

            public class Shop {
                public static int total(String item, int qty, double price) {
                    print(${'$'}"${'$'}{qty} x ${'$'}item");
                    return qty;
                }
            }
            """.trimIndent(),
        )
        val app = myFixture.addFileToProject(
            "app/App.jux",
            """
            package app;

            import shop.Shop;

            void main() {
                print(Shop.total("tea", 2, 1.5));
            }
            """.trimIndent(),
        )
        JuxRefactoringInput.answers[JuxIntroduceParameterObjectHandler.TITLE] = "Line"
        JuxRefactoringInput.answers[JuxIntroduceParameterObjectHandler.PARAMETERS] = "item, qty"
        val method = PsiTreeUtil.findChildrenOfType(shop, JuxMethodDeclaration::class.java).first()
        JuxIntroduceParameterObjectHandler().invoke(project, arrayOf(method), null)

        assertEquals(
            """
            package shop;

            public class Shop {
                public static int total(Line line, double price) {
                    print(${'$'}"${'$'}{line.qty} x ${'$'}{line.item}");
                    return line.qty;
                }
            }
            """.trimIndent(),
            shop.text,
        )
        assertEquals(
            """
            package app;

            import shop.Shop;
            import shop.Line;

            void main() {
                print(Shop.total(new Line("tea", 2), 1.5));
            }
            """.trimIndent(),
            app.text,
        )
        val record = shop.containingDirectory.findFile("Line.jux")
        assertNotNull("the record gets its own file", record)
        assertEquals("package shop;\n\npublic record Line(String item, int qty) {}\n", record!!.text)
    }

    fun testAnAssignedParameterIsRefused() {
        myFixture.configureByText("a.jux", "int f(int a, int b) { a = a + 1; return a + b; }")
        val method = PsiTreeUtil.findChildrenOfType(myFixture.file, JuxMethodDeclaration::class.java).first()
        try {
            JuxIntroduceParameterObjectHandler.plan(method, "Pair2", listOf(0, 1))
        } catch (e: JuxExtractMethodHandler.Refusal) {
            assertTrue(e.message, e.message!!.contains("final"))
            return
        }
        fail("expected a refusal")
    }
}
