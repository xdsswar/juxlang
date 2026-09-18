package dev.jux.intellij.format

import com.intellij.application.options.CodeStyle
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.psi.codeStyle.LanguageCodeStyleSettingsProvider
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.editor.JuxImportOptimizer
import dev.jux.intellij.psi.JuxFile

/**
 * The Imports tab of Code Style | Jux: the group layout, the blank line
 * between groups, and the wildcard threshold, as Optimize Imports and
 * auto-import apply them.
 */
class JuxImportsCodeStyleTest : BasePlatformTestCase() {

    private val style get() = CodeStyle.getSettings(project).getCustomSettings(JuxCodeStyleSettings::class.java)

    override fun tearDown() {
        try {
            style.IMPORT_LAYOUT = JuxCodeStyleSettings.DEFAULT_LAYOUT
            style.BLANK_LINE_BETWEEN_IMPORT_GROUPS = true
            style.NAMES_COUNT_TO_USE_WILDCARD = 0
        } finally {
            super.tearDown()
        }
    }

    private fun optimize(code: String): String {
        myFixture.configureByText("a.jux", code.trimIndent())
        WriteCommandAction.runWriteCommandAction(project) {
            JuxImportOptimizer().processFile(myFixture.file).run()
        }
        return myFixture.editor.document.text
    }

    private val source = """
        package demo;

        import shop.Money;
        import rust.std.Vec;
        import shop.Basket;

        void main() {
            Vec<int> v = new Vec<int>();
            Money m = new Money(1);
            Basket b = new Basket();
        }
    """

    fun testTheDefaultsKeepTheCorpusLayout() {
        assertEquals(JuxCodeStyleSettings.DEFAULT_LAYOUT, style.IMPORT_LAYOUT)
        assertEquals(0, style.NAMES_COUNT_TO_USE_WILDCARD)
        val out = optimize(source)
        assertTrue(out, out.contains("import rust.std.Vec;\n\nimport shop.Basket;\nimport shop.Money;"))
    }

    fun testALayoutPuttingTheProjectFirstAndNoBlankLines() {
        style.IMPORT_LAYOUT = "shop.;*"
        style.BLANK_LINE_BETWEEN_IMPORT_GROUPS = false
        val out = optimize(source)
        assertTrue(out, out.contains("import shop.Basket;\nimport shop.Money;\nimport rust.std.Vec;"))
    }

    fun testTheWildcardThresholdCollapsesAPackage() {
        style.NAMES_COUNT_TO_USE_WILDCARD = 2
        val out = optimize(source)
        assertTrue(out, out.contains("import rust.std.Vec;\n\nimport shop.*;"))
        assertFalse(out, out.contains("import shop.Money;"))
    }

    fun testAutoImportUsesTheWildcardAtTheThreshold() {
        style.NAMES_COUNT_TO_USE_WILDCARD = 2
        myFixture.configureByText("a.jux", "package demo;\n\nimport shop.Money;\n\nvoid main() {}\n")
        WriteCommandAction.runWriteCommandAction(project) {
            JuxAutoImport.addImport(project, myFixture.editor.document, myFixture.file as JuxFile, "shop.Basket", "Basket")
        }
        assertEquals("package demo;\n\nimport shop.*;\n\nvoid main() {}\n", myFixture.editor.document.text)
    }

    fun testLayoutParsing() {
        assertEquals(listOf(listOf("rust.", "c.", "cpp."), listOf("jux."), listOf("*")), JuxCodeStyleSettings.parseLayout("rust.|c.|cpp.;jux.;*"))
        // A layout without a catch-all still places every import.
        assertEquals(listOf(listOf("a."), listOf("*")), JuxCodeStyleSettings.parseLayout("a."))
        val groups = JuxCodeStyleSettings.parseLayout("a.;a.b.;*")
        assertEquals("the longest prefix wins", 1, JuxCodeStyleSettings.groupIndex(groups, "a.b.C"))
        assertEquals(2, JuxCodeStyleSettings.groupIndex(groups, "z.Y"))
    }

    fun testThePageHasAnImportsTab() {
        val provider = LanguageCodeStyleSettingsProvider.forLanguage(JuxLanguage)!!
        assertTrue(provider.createCustomSettings(CodeStyle.getSettings(project)) is JuxCodeStyleSettings)
        val panel = JuxImportsCodeStylePanel(CodeStyle.getSettings(project))
        try {
            assertEquals("Imports", panel.tabTitle)
            assertFalse(panel.isModified(CodeStyle.getSettings(project)))
        } finally {
            panel.dispose()
        }
    }
}
