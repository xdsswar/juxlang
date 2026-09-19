package dev.jux.intellij.folding

import com.intellij.codeInsight.folding.CodeFoldingSettings
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Folding regions and their default collapse state, against Java's: the
 * imports and the file header start collapsed; method bodies, doc comments
 * and the rest follow their settings, which start expanded.
 */
class JuxFoldingTest : BasePlatformTestCase() {

    /** One fold: what it hides, what it shows instead, and whether it starts collapsed. */
    data class Fold(val text: String, val placeholder: String, val collapsed: Boolean)

    /**
     * The regions [text] folds into. The platform never collapses a region
     * holding the caret, so a test that checks a collapsed default puts its
     * `<caret>` outside that region.
     */
    private fun folds(text: String): List<Fold> {
        myFixture.configureByText("a.jux", text)
        com.intellij.testFramework.EditorTestUtil.buildInitialFoldingsInBackground(myFixture.editor)
        val doc = myFixture.editor.document
        return myFixture.editor.foldingModel.allFoldRegions.map {
            Fold(doc.getText(it.textRange), it.placeholderText, !it.isExpanded)
        }
    }

    private fun fold(folds: List<Fold>, placeholder: String): Fold =
        folds.firstOrNull { it.placeholder == placeholder } ?: error("no fold shown as $placeholder in $folds")

    private val program = """
        // Copyright header
        // second line
        package shop;

        import a.B;
        import c.D;

        /**
         * Keeps the till. Counts every sale.
         * @param x ignored
         */
        public class Till {<caret>
            public int Total {
                get;
                set;
            }
            public void ring() {
                var f = () -> {
                    print("hi");
                };
            }
            public class Drawer {
                int coins;
            }
        }
    """.trimIndent()

    fun testImportsAndHeaderStartCollapsed() {
        val f = folds(program)
        val imports = fold(f, "...")
        assertEquals("a.B;\nimport c.D;", imports.text)
        assertTrue("imports start collapsed, as Java's do", imports.collapsed)
        val header = fold(f, "//...")
        assertTrue(header.text.startsWith("// Copyright"))
        assertTrue("the file header starts collapsed", header.collapsed)
    }

    fun testDocCommentShowsItsFirstSentence() {
        val doc = fold(folds(program), "/** Keeps the till */")
        assertFalse("documentation starts expanded", doc.collapsed)
    }

    fun testBodiesStartExpanded() {
        val f = folds(program)
        val blocks = f.filter { it.placeholder == "{...}" }
        // Till's body, the accessor list, ring's body, the lambda's body, Drawer's body.
        assertEquals(f.toString(), 5, blocks.size)
        assertTrue(blocks.none { it.collapsed })
    }

    fun testSettingsCollapseTheirRegions() {
        val shared = CodeFoldingSettings.getInstance()
        val jux = JuxCodeFoldingSettings.getInstance()
        val saved = listOf(shared.COLLAPSE_METHODS, jux.collapseLambdas, jux.collapseInnerClasses, jux.collapseAccessors)
        try {
            shared.COLLAPSE_METHODS = true
            jux.collapseLambdas = true
            jux.collapseInnerClasses = true
            jux.collapseAccessors = true
            val collapsed = folds(program).filter { it.placeholder == "{...}" && it.collapsed }.map { it.text.lines().first().trim() }
            // ring's body, the lambda's, Drawer's, the accessors; not Till's own body.
            assertEquals(collapsed.toString(), 4, collapsed.size)
        } finally {
            shared.COLLAPSE_METHODS = saved[0]
            jux.collapseLambdas = saved[1]
            jux.collapseInnerClasses = saved[2]
            jux.collapseAccessors = saved[3]
        }
    }

    fun testCommentRunsAndBlockComments() {
        val f = folds(
            """
            class A {
                // one
                // two
                void m() {}
                /* a
                   b */
                void n() {}
                // lone
            }
            """.trimIndent(),
        )
        assertEquals("// one\n    // two", fold(f, "//...").text)
        assertFalse(fold(f, "/*...*/").collapsed)
        assertEquals("one run, the lone comment folds nothing: $f", 1, f.count { it.placeholder == "//..." })
    }

    fun testLongStringLiteral() {
        val long = "x".repeat(100)
        val f = folds("void main() { print(\"$long\"); }")
        val s = fold(f, "...\"")
        assertEquals(100 + 2 - 40, s.text.length)
        assertFalse(s.collapsed)
    }

    fun testOptionsPageIsListedWithTheOthers() {
        val ep = com.intellij.openapi.extensions.ExtensionPointName<com.intellij.openapi.options.ConfigurableEP<*>>(
            "com.intellij.codeFoldingOptionsProvider",
        )
        assertTrue(
            "the Jux page is registered",
            ep.extensionList.any { it.instanceClass == JuxCodeFoldingOptionsProvider::class.java.name },
        )
        // Building it binds every checkbox to its setting.
        assertNotNull(JuxCodeFoldingOptionsProvider())
    }

    fun testSingleImportDoesNotFold() {
        val f = folds("import a.B;\nclass C {}")
        assertTrue(f.toString(), f.none { it.placeholder == "..." })
    }
}
