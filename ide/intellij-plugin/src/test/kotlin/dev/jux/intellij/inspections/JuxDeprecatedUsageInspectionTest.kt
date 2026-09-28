package dev.jux.intellij.inspections

import com.intellij.codeInsight.daemon.impl.HighlightInfo
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * W0491 (ERRATA E132): a call to a `@Deprecated` declaration is struck through
 * with the declaration's message, whether the program declares it or a crate's
 * stub marks it (`@Deprecated(message = "...")` from `#[deprecated]`).
 */
class JuxDeprecatedUsageInspectionTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(JuxDeprecatedUsageInspection())
    }

    private fun w0491(code: String): List<HighlightInfo> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting().filter { it.description?.contains("W0491") == true }
    }

    fun testACallToAnOwnDeprecatedMethod() {
        val found = w0491(
            """
            class Panel {
                @Deprecated(message = "Renamed to `show`")
                public void show_inside() { }
                public void show() { }
            }
            @Deprecated
            void legacy() { }
            void main() {
                var p = new Panel();
                p.show_inside();
                p.show();
                legacy();
            }
            """,
        )
        val texts = found.map { it.description }
        assertEquals(texts.toString(), 2, found.size)
        assertTrue(texts.toString(), texts.contains("'Panel.show_inside' is deprecated: Renamed to `show` (W0491)"))
        assertTrue(texts.toString(), texts.contains("'legacy' is deprecated (W0491)"))
        // Struck through, as Java shows a deprecated call.
        val deprecated = com.intellij.openapi.editor.colors.CodeInsightColors.DEPRECATED_ATTRIBUTES
        assertTrue(found.all { it.type.attributesKey == deprecated || it.forcedTextAttributesKey == deprecated })
        assertEquals(listOf("show_inside", "legacy"), found.sortedBy { it.startOffset }.map { myFixture.file.text.substring(it.startOffset, it.endOffset) })
    }

    fun testANewOfADeprecatedConstructor() {
        val found = w0491(
            """
            class Old {
                @Deprecated("use Old.of()")
                public Old() { }
                public Old(int x) { }
            }
            void main() {
                var a = new Old();
                var b = new Old(1);
            }
            """,
        )
        assertEquals(listOf("'new Old' is deprecated: use Old.of() (W0491)"), found.map { it.description })
    }

    fun testACrateStubsDeprecatedMember() {
        myFixture.addFileToProject(
            "stubs/rust/g37ui.jux.d",
            """
            package rust.g37ui;

            public class Panel {
                public Panel();
                @Deprecated(message = "Renamed to `show`") public void show_inside();
            }
            """.trimIndent(),
        )
        val found = w0491(
            """
            import rust.g37ui.Panel;

            void main() {
                new Panel().show_inside();
            }
            """,
        )
        assertEquals(listOf("'Panel.show_inside' is deprecated: Renamed to `show` (W0491)"), found.map { it.description })
    }
}
