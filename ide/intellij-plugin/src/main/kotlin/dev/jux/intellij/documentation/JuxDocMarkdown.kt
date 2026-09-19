package dev.jux.intellij.documentation

import com.intellij.openapi.util.text.StringUtil

/**
 * Doc-comment Markdown to HTML, the same subset and the same reading `jux doc`
 * uses for its pages (juxc-driver docgen `markdown_to_html` / `inline`), so
 * Quick Documentation (Ctrl+Q) shows a comment the way the generated site
 * does:
 *
 *  - paragraphs (a blank line separates), `#` headings, `-`/`*` bullet and
 *    `1.` numbered lists with indented continuation lines;
 *  - fenced blocks: a ```` ```jux ```` block goes through [highlight] (the
 *    editor's colors), others are plain `<pre>`; an `ignore` / `no_run`
 *    flag is captioned the way the doc examples treat it;
 *  - inline `code`, **bold**, *italic* / _italic_, and [text](url) links,
 *    plus the Javadoc forms `{@code x}` and `{@link Type#member label}` that
 *    Jux comments written by Java hands still carry.
 *
 * Pure apart from [highlight], which the caller supplies (the provider passes
 * the IDE's syntax-highlighted HTML; tests pass a plain escaper).
 */
object JuxDocMarkdown {

    /** Renders one code block's text as HTML. */
    fun interface Highlighter {
        fun html(code: String): String
    }

    /** The default: escaped, uncolored. */
    val PLAIN = Highlighter { StringUtil.escapeXmlEntities(it) }

    fun toHtml(md: String, highlight: Highlighter = PLAIN): String {
        val out = StringBuilder()
        val para = ArrayList<String>()
        var list: Pair<String, MutableList<String>>? = null
        var fence: Pair<String, MutableList<String>>? = null

        fun flushPara() {
            if (para.isNotEmpty()) {
                out.append("<p>").append(inline(para.joinToString(" "))).append("</p>")
                para.clear()
            }
        }
        fun flushList() {
            list?.let { (tag, items) ->
                out.append('<').append(tag).append('>')
                items.forEach { out.append("<li>").append(inline(it)).append("</li>") }
                out.append("</").append(tag).append('>')
            }
            list = null
        }
        fun emitFence(info: String, lines: List<String>) {
            val words = info.split(Regex("[\\s,]+")).filter { it.isNotEmpty() }
            val lang = words.firstOrNull()?.lowercase().orEmpty()
            val code = lines.joinToString("\n")
            out.append("<pre><code>")
            out.append(if (lang == "jux") highlight.html(code) else StringUtil.escapeXmlEntities(code))
            out.append("</code></pre>")
            if (lang == "jux") {
                when {
                    "ignore" in words -> out.append("<p><i>Example shown only: jux test --doc does not compile it.</i></p>")
                    "no_run" in words -> out.append("<p><i>Example compiled by jux test --doc, not run.</i></p>")
                }
            }
        }

        for (line in md.lines()) {
            val trimmed = line.trim()
            val open = fence
            if (open != null) {
                if (trimmed.startsWith("```")) {
                    emitFence(open.first, open.second)
                    fence = null
                } else {
                    open.second.add(line)
                }
                continue
            }
            if (trimmed.startsWith("```")) {
                flushPara(); flushList()
                fence = trimmed.removePrefix("```").trim() to ArrayList()
                continue
            }
            if (trimmed.isEmpty()) {
                flushPara(); flushList()
                continue
            }
            val hashes = trimmed.takeWhile { it == '#' }.length
            if (hashes in 1..6 && trimmed.getOrNull(hashes) == ' ') {
                flushPara(); flushList()
                // Headings inside an item's docs sit below the popup's own title.
                val level = (hashes + 3).coerceAtMost(6)
                out.append("<h$level>").append(inline(trimmed.substring(hashes).trim())).append("</h$level>")
                continue
            }
            val bullet = trimmed.removePrefixOrNull("- ") ?: trimmed.removePrefixOrNull("* ")
            val numbered = NUMBERED.matchEntire(trimmed)?.groupValues?.get(1)
            val item = bullet ?: numbered
            if (item != null) {
                flushPara()
                val tag = if (bullet != null) "ul" else "ol"
                val current = list
                if (current != null && current.first == tag) {
                    current.second.add(item)
                } else {
                    flushList()
                    list = tag to mutableListOf(item)
                }
                continue
            }
            val currentList = list
            if (currentList != null) {
                // An indented line continues the last list item.
                val items = currentList.second
                if (line.startsWith("  ") && items.isNotEmpty()) {
                    items[items.size - 1] = items.last() + " " + trimmed
                    continue
                }
                flushList()
            }
            para.add(trimmed)
        }
        fence?.let { (info, lines) -> emitFence(info, lines) }
        flushPara()
        flushList()
        return out.toString()
    }

    /** Inline Markdown and the Javadoc inline tags, escaped. */
    fun inline(text: String): String {
        val out = StringBuilder()
        var i = 0
        while (i < text.length) {
            val c = text[i]
            if (c == '{' && text.startsWith("{@", i)) {
                JAVADOC_INLINE.matchAt(text, i)?.let { m ->
                    val kind = m.groupValues[1]
                    val body = m.groupValues[2].trim()
                    val shown = if (kind.startsWith("link")) {
                        body.substringAfter(' ', "").trim().ifEmpty { body.substringBefore(' ').replace('#', '.') }
                    } else {
                        body
                    }
                    out.append("<code>").append(StringUtil.escapeXmlEntities(shown)).append("</code>")
                    i = m.range.last + 1
                    continue
                }
            }
            if (c == '`') {
                val close = text.indexOf('`', i + 1)
                if (close > i) {
                    out.append("<code>").append(StringUtil.escapeXmlEntities(text.substring(i + 1, close))).append("</code>")
                    i = close + 1
                    continue
                }
            }
            if (c == '*' && text.startsWith("**", i)) {
                val close = text.indexOf("**", i + 2)
                if (close > i + 2) {
                    out.append("<b>").append(inline(text.substring(i + 2, close))).append("</b>")
                    i = close + 2
                    continue
                }
            }
            if ((c == '*' || c == '_') && i + 1 < text.length && !text[i + 1].isWhitespace()) {
                val close = text.indexOf(c, i + 1)
                if (close > i + 1 && !text[close - 1].isWhitespace()) {
                    out.append("<i>").append(inline(text.substring(i + 1, close))).append("</i>")
                    i = close + 1
                    continue
                }
            }
            if (c == '[') {
                LINK.matchAt(text, i)?.let { m ->
                    out.append("<a href=\"").append(StringUtil.escapeXmlEntities(m.groupValues[2])).append("\">")
                        .append(inline(m.groupValues[1])).append("</a>")
                    i = m.range.last + 1
                    continue
                }
            }
            out.append(StringUtil.escapeXmlEntities(c.toString()))
            i++
        }
        return out.toString()
    }

    private fun String.removePrefixOrNull(prefix: String): String? = if (startsWith(prefix)) substring(prefix.length) else null

    private val NUMBERED = Regex("""^\d+\. (.*)$""")
    private val JAVADOC_INLINE = Regex("""\{@(code|literal|link|linkplain)\s+([^}]*)}""")
    private val LINK = Regex("""\[([^\]]*)]\(([^)]*)\)""")
}
