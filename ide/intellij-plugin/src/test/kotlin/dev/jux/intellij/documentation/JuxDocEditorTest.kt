package dev.jux.intellij.documentation

import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.highlight.JuxSyntaxHighlighter

/**
 * Doc comments in the editor: tag and parameter completion, the coloring
 * of tags and ```` ```jux ```` example blocks, and the Quick Documentation
 * rendering of `@deprecated` and example blocks.
 */
class JuxDocEditorTest : BasePlatformTestCase() {

    fun testTagCompletion() {
        myFixture.configureByText(
            "a.jux",
            """
            /**
             * Adds.
             * @dep<caret>
             */
            public int add(int a, int b) { return a + b; }
            """.trimIndent(),
        )
        myFixture.completeBasic()
        assertTrue(myFixture.editor.document.text, myFixture.editor.document.text.contains("@deprecated"))
    }

    fun testParamCompletionOffersUndocumentedParameters() {
        myFixture.configureByText(
            "a.jux",
            """
            /**
             * Adds.
             * @param a the first
             * @param <caret>
             */
            public int add(int a, int b) { return a + b; }
            """.trimIndent(),
        )
        val items = myFixture.completeBasic()?.map { it.lookupString }
        // One candidate left (`b`) is inserted directly.
        if (items == null) {
            assertTrue(myFixture.editor.document.text, myFixture.editor.document.text.contains("@param b"))
        } else {
            assertEquals(listOf("b"), items)
        }
    }

    fun testFenceInfoCompletion() {
        myFixture.configureByText(
            "a.jux",
            """
            /**
             * Adds.
             * ```jux n<caret>
             */
            public int add(int a, int b) { return a + b; }
            """.trimIndent(),
        )
        val items = myFixture.completeBasic()?.map { it.lookupString }
        if (items == null) {
            assertTrue(myFixture.editor.document.text, myFixture.editor.document.text.contains("```jux no_run"))
        } else {
            assertTrue(items.toString(), items.contains("jux no_run"))
        }
    }

    fun testTagsAndExampleCodeAreColored() {
        myFixture.configureByText(
            "a.jux",
            """
            /**
             * Adds.
             *
             * ```jux no_run
             * var total = add(1, 2);
             * ```
             * @param a the first
             */
            public int add(int a, int b) { return a + b; }
            """.trimIndent(),
        )
        val text = myFixture.editor.document.text
        val infos = myFixture.doHighlighting()
        fun keyAt(word: String) = infos.filter { text.substring(it.startOffset, it.endOffset) == word }
            .mapNotNull { it.forcedTextAttributesKey }
        assertTrue(keyAt("@param").toString(), DefaultLanguageHighlighterColors.DOC_COMMENT_TAG in keyAt("@param"))
        assertTrue(DefaultLanguageHighlighterColors.DOC_COMMENT_TAG_VALUE in keyAt("a"))
        assertTrue(DefaultLanguageHighlighterColors.METADATA in keyAt("no_run"))
        // `var` inside the example gets the editor's keyword color.
        assertTrue(keyAt("var").toString(), JuxSyntaxHighlighter.KEYWORD in keyAt("var"))
    }

    fun testQuickDocShowsDeprecationAndExamples() {
        myFixture.configureByText(
            "a.jux",
            """
            /**
             * Doubles a number.
             *
             * ```jux
             * var four = twice(2);
             * ```
             * @deprecated use `times`
             */
            public int twice(int n) { return n * 2; }
            void main() { tw<caret>ice(1); }
            """.trimIndent(),
        )
        val target = myFixture.file.findReferenceAt(myFixture.caretOffset)?.resolve()
        val doc = JuxDocumentationProvider().generateDoc(target, null) ?: ""
        assertTrue(doc, doc.contains("<b>Deprecated</b>: use <code>times</code>"))
        assertTrue(doc, doc.contains("<pre><code>"))
        assertTrue(doc, doc.contains("four"))
        assertFalse("the tag is not in the description: $doc", doc.contains("@deprecated"))
    }
}
