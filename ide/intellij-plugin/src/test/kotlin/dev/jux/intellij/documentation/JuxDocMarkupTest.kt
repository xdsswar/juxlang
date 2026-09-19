package dev.jux.intellij.documentation

import junit.framework.TestCase

/**
 * The pure doc-comment readers: fenced blocks and tags inside a raw comment
 * ([JuxDocFences]), the tag split with `@deprecated` and fences
 * ([JuxDocComment]), and the Markdown renderer shared with `jux doc`
 * ([JuxDocMarkdown]).
 */
class JuxDocMarkupTest : TestCase() {

    private val comment = """
        /**
         * Doubles `n`.
         *
         * ```jux no_run
         * @Test
         * var x = twice(2);
         * ```
         * @param n the number
         * @deprecated use `times`
         */
    """.trimIndent()

    fun testFencesKnowTheirInfoAndCodeLines() {
        val fence = JuxDocFences.fences(comment).single()
        assertEquals("jux", fence.language)
        assertTrue(fence.isJux)
        assertEquals(listOf("jux", "no_run"), fence.info.map { it.text })
        assertEquals(listOf("@Test", "var x = twice(2);"), fence.lines.map { comment.substring(it.first, it.last + 1) })
        // Spans point into the raw comment.
        val flag = fence.info[1]
        assertEquals("no_run", comment.substring(flag.start, flag.end))
    }

    fun testTagsSkipTheFenceAndCarryTheirName() {
        val tags = JuxDocFences.tags(comment)
        assertEquals(listOf("param", "deprecated"), tags.map { it.name })
        assertEquals("@param", comment.substring(tags[0].start, tags[0].end))
        assertEquals("n", tags[0].value?.text)
        assertNull(tags[1].value)
    }

    fun testParseKeepsFencesInTheDescription() {
        val doc = JuxDocComment.parse(
            """
            Doubles `n`.

            ```jux
            @Test
            var x = twice(2);
            ```
            @param n the number
            @deprecated use `times`
            """.trimIndent(),
        )
        assertEquals(listOf("n" to "the number"), doc.params)
        assertEquals("use `times`", doc.deprecated)
        assertTrue(doc.description, doc.description.contains("@Test"))
        assertTrue(doc.description, doc.description.contains("```jux"))
    }

    fun testATagEndsAtABlankLine() {
        val doc = JuxDocComment.parse("@return the sum\n\nMore description.")
        assertEquals("the sum", doc.returns)
        assertTrue(doc.description, doc.description.contains("More description."))
    }

    fun testMarkdownLikeJuxDoc() {
        val html = JuxDocMarkdown.toHtml(
            """
            First **bold** and *em* and `code` and [site](https://jux.dev).

            # Usage
            - one
            - two
              continued
            1. first

            ```jux ignore
            var a = 1 < 2;
            ```
            """.trimIndent(),
        )
        assertTrue(html, html.contains("<p>First <b>bold</b> and <i>em</i> and <code>code</code> and <a href=\"https://jux.dev\">site</a>.</p>"))
        assertTrue(html, html.contains("<h4>Usage</h4>"))
        assertTrue(html, html.contains("<ul><li>one</li><li>two continued</li></ul>"))
        assertTrue(html, html.contains("<ol><li>first</li></ol>"))
        assertTrue(html, html.contains("<pre><code>var a = 1 &lt; 2;</code></pre>"))
        assertTrue(html, html.contains("does not compile it"))
    }

    fun testJavadocInlineTagsStillRender() {
        assertEquals("<code>qty</code> and <code>Cart.total</code>", JuxDocMarkdown.inline("{@code qty} and {@link Cart#total}"))
        assertEquals("<code>the total</code>", JuxDocMarkdown.inline("{@link Cart#total the total}"))
    }

    fun testJuxBlocksGoThroughTheHighlighter() {
        val html = JuxDocMarkdown.toHtml("```jux\nvar x = 1;\n```") { "[[${it}]]" }
        assertTrue(html, html.contains("<pre><code>[[var x = 1;]]</code></pre>"))
        val other = JuxDocMarkdown.toHtml("```text\n<b>\n```") { "never" }
        assertTrue(other, other.contains("&lt;b&gt;"))
    }
}
