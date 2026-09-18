package dev.jux.intellij.editor

import com.intellij.openapi.command.WriteCommandAction
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.inspections.JuxUnusedImportInspection
import dev.jux.intellij.settings.JuxAutoImportOptionsProvider
import dev.jux.intellij.settings.JuxCodeInsightSettings
import dev.jux.intellij.settings.JuxCodeInsightWorkspaceSettings

/**
 * Optimize Imports and the unused-import inspection, which share one analysis
 * ([JuxImportSupport.analyze]) and must therefore agree.
 */
class JuxOptimizeImportsTest : BasePlatformTestCase() {

    private fun optimize(code: String): String {
        myFixture.configureByText("a.jux", code.trimIndent())
        WriteCommandAction.runWriteCommandAction(project) {
            JuxImportOptimizer().processFile(myFixture.file).run()
        }
        return myFixture.editor.document.text
    }

    fun testUnusedRemovedAndSurvivorsGroupedLikeJava() {
        val out = optimize(
            """
            package demo;

            import shop.money.Money;
            import jux.std.testing.assertEqual;
            import rust.std.Vec;
            import shop.unused.Nothing;
            import rust.std.HashMap;

            public void main() {
                Vec<int> v = new Vec<int>();
                HashMap<String, int> m = new HashMap<String, int>();
                Money cash = new Money(1);
                assertEqual(1, 1);
            }
            """,
        )
        assertTrue(
            out,
            out.contains(
                """
                import rust.std.HashMap;
                import rust.std.Vec;

                import jux.std.testing.assertEqual;

                import shop.money.Money;
                """.trimIndent(),
            ),
        )
        assertFalse(out, out.contains("Nothing"))
    }

    fun testGroupedImportKeepsOnlyUsedMembers() {
        val out = optimize(
            """
            package demo;

            import shop.{Cart, Item, Order};

            public void main() {
                Cart c = new Cart();
                Order o = new Order();
            }
            """,
        )
        assertTrue(out, out.contains("import shop.{Cart, Order};"))
        assertFalse(out, out.contains("Item"))
    }

    fun testGroupWithOneUsedMemberBecomesSingleImport() {
        val out = optimize(
            """
            package demo;

            import shop.{Cart, Item as Thing};

            public void main() {
                Thing t = new Thing();
            }
            """,
        )
        assertTrue(out, out.contains("import shop.Item as Thing;"))
        assertFalse(out, out.contains("Cart"))
    }

    fun testDuplicateRemoved() {
        val out = optimize(
            """
            package demo;

            import shop.Cart;
            import shop.Cart;

            public void main() {
                Cart c = new Cart();
            }
            """,
        )
        assertEquals(1, Regex("import shop\\.Cart;").findAll(out).count())
    }

    fun testWildcardProvablyUnusedIsRemovedAndUsedOneKept() {
        myFixture.addFileToProject("util/Helper.jux", "package util;\npublic class Helper {}\n")
        myFixture.addFileToProject("other/Other.jux", "package other;\npublic class Other {}\n")
        val out = optimize(
            """
            package demo;

            import util.*;
            import other.*;

            public void main() {
                Helper h = new Helper();
            }
            """,
        )
        assertTrue(out, out.contains("import util.*;"))
        assertFalse(out, out.contains("import other.*;"))
    }

    fun testWildcardKeptWhenANameCannotBePlaced() {
        // `Mystery` is declared nowhere the index can see, so it might come
        // from the wildcard: keeping the line is the safe answer.
        val out = optimize(
            """
            package demo;

            import somewhere.*;

            public void main() {
                Mystery m = new Mystery();
            }
            """,
        )
        assertTrue(out, out.contains("import somewhere.*;"))
    }

    fun testAlreadyOptimalFileIsUntouched() {
        val code =
            """
            package demo;

            import rust.std.Vec;

            public void main() {
                Vec<int> v = new Vec<int>();
            }
            """.trimIndent()
        assertEquals(code, optimize(code))
    }

    fun testInspectionMarksTheUnusedMemberOfAGroup() {
        myFixture.enableInspections(JuxUnusedImportInspection())
        myFixture.configureByText(
            "a.jux",
            """
            package demo;

            import shop.{Cart, Item};

            public void main() {
                Cart c = new Cart();
            }
            """.trimIndent(),
        )
        val warnings = myFixture.doHighlighting().filter { it.description?.startsWith("Unused import") == true }
        assertEquals(listOf("Unused import 'Item'"), warnings.map { it.description })
        assertEquals("Item", warnings.single().text)
    }

    fun testOptimizeImportsQuickFix() {
        myFixture.enableInspections(JuxUnusedImportInspection())
        myFixture.configureByText(
            "a.jux",
            """
            package demo;

            import shop.Unused;
            import shop.Cart;

            public void main() {
                Cart c = new Cart();
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        myFixture.editor.caretModel.moveToOffset(myFixture.file.text.indexOf("Unused"))
        myFixture.launchAction(myFixture.findSingleIntention("Optimize imports"))
        val out = myFixture.editor.document.text
        assertFalse(out, out.contains("Unused"))
        assertTrue(out, out.contains("import shop.Cart;"))
    }

    fun testEnableOnTheFlyFixTurnsTheOptionOnAndOptimizes() {
        val settings = JuxCodeInsightWorkspaceSettings.getInstance(project)
        settings.optimizeImportsOnTheFly = false
        try {
            myFixture.enableInspections(JuxUnusedImportInspection())
            myFixture.configureByText(
                "a.jux",
                "package demo;\n\nimport shop.Unused;\n\npublic void main() {}\n",
            )
            myFixture.doHighlighting()
            myFixture.editor.caretModel.moveToOffset(myFixture.file.text.indexOf("Unused"))
            myFixture.launchAction(myFixture.findSingleIntention("Enable 'Optimize imports on the fly'"))
            assertTrue(settings.optimizeImportsOnTheFly)
            assertFalse(myFixture.editor.document.text.contains("Unused"))
        } finally {
            settings.optimizeImportsOnTheFly = false
        }
    }

    fun testNotTimeToOptimizeWhileTheCaretIsOnTheImports() {
        myFixture.configureByText(
            "a.jux",
            "package demo;\n\nimport shop.Unused;<caret>\n\npublic void main() {}\n",
        )
        assertFalse(JuxOnTheFlyImportOptimizer.isTimeToOptimize(myFixture.file, myFixture.editor))
    }

    fun testAutoImportPageHasTheJuxOptions() {
        val app = JuxCodeInsightSettings.getInstance()
        val before = app.addUnambiguousImportsOnTheFly
        val provider = JuxAutoImportOptionsProvider(project)
        val component = provider.createComponent()
        assertNotNull(component)
        provider.reset()
        assertFalse(provider.isModified)
        provider.disposeUIResources()
        assertEquals(before, app.addUnambiguousImportsOnTheFly)
    }
}
