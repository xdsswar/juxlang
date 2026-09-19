package dev.jux.intellij.documentation

/**
 * Where things are inside the raw text of one `/** ... */` doc comment, as
 * offsets into that text: the fenced code blocks (with their info string and
 * each code line's span after the ` * ` margin) and the block tags. Pure, so
 * the highlighting annotator and its tests share one reading.
 *
 * The rules are `jux doc`'s (juxc-driver docgen): a fence opens and closes on
 * a line starting with ```` ``` ````, the info string's first word is the
 * language, `ignore` and `no_run` are flags (a comma separates as well as a
 * space), and a `@tag` counts only outside a fence.
 */
object JuxDocFences {

    /** The doc tags `jux doc` understands (docgen `parse_doc_comment`). */
    val TAGS: List<String> = listOf("param", "return", "returns", "throws", "exception", "deprecated", "since", "see")

    /** The flags a ```` ```jux ```` block may carry. */
    val FLAGS: List<String> = listOf("ignore", "no_run")

    /** One word of a fence's info string, with its span. */
    data class Word(val text: String, val start: Int, val end: Int)

    /** One fenced block. */
    data class Fence(
        /** The info string's words (`jux`, `no_run`), with spans. */
        val info: List<Word>,
        /** The span of each code line's content, after the comment margin. */
        val lines: List<IntRange>,
    ) {
        /** The block's language, lower-cased (`jux`), or empty. */
        val language: String get() = info.firstOrNull()?.text?.lowercase().orEmpty()

        /** True when the block is Jux code the doc examples compile. */
        val isJux: Boolean get() = language == "jux"
    }

    /** A `@tag` with its span, and for `@param` / `@throws` the name after it. */
    data class Tag(val name: String, val start: Int, val end: Int, val value: Word?)

    /**
     * Every line of [text] as the span of its content after the comment
     * opener or the leading star, and one space of margin.
     */
    private fun contentSpans(text: String): List<IntRange> {
        val out = ArrayList<IntRange>()
        var lineStart = 0
        while (lineStart <= text.length) {
            val nl = text.indexOf('\n', lineStart).let { if (it < 0) text.length else it }
            var s = lineStart
            val lineEnd = if (nl > lineStart && text[nl - 1] == '\r') nl - 1 else nl
            while (s < lineEnd && text[s].isWhitespace()) s++
            if (text.startsWith("/**", s)) s += 3
            else if (s < lineEnd && text[s] == '*' && !text.startsWith("*/", s)) s++
            if (s < lineEnd && text[s] == ' ') s++
            var e = lineEnd
            if (text.substring(s, e).trimEnd().endsWith("*/")) e = s + text.substring(s, e).trimEnd().length - 2
            out.add(s until maxOf(s, e))
            lineStart = nl + 1
        }
        return out
    }

    /** The fenced blocks of a doc comment's raw [text]. */
    fun fences(text: String): List<Fence> {
        val out = ArrayList<Fence>()
        var info: List<Word>? = null
        var lines = ArrayList<IntRange>()
        for (span in contentSpans(text)) {
            val content = text.substring(span.first, span.last + 1)
            val lead = content.length - content.trimStart().length
            if (content.trimStart().startsWith("```")) {
                if (info == null) {
                    val infoStart = span.first + lead + 3
                    info = words(text, infoStart, span.last + 1)
                    lines = ArrayList()
                } else {
                    out.add(Fence(info, lines))
                    info = null
                }
                continue
            }
            if (info != null && !span.isEmpty()) lines.add(span)
        }
        return out
    }

    /** The block tags of a doc comment's raw [text], outside fenced blocks. */
    fun tags(text: String): List<Tag> {
        val out = ArrayList<Tag>()
        var inFence = false
        for (span in contentSpans(text)) {
            if (span.isEmpty()) continue
            val content = text.substring(span.first, span.last + 1)
            val trimmed = content.trimStart()
            if (trimmed.startsWith("```")) {
                inFence = !inFence
                continue
            }
            if (inFence || !trimmed.startsWith("@")) continue
            val start = span.first + (content.length - trimmed.length)
            val name = trimmed.drop(1).takeWhile { it.isLetter() }
            if (name !in TAGS) continue
            val end = start + 1 + name.length
            val value = if (name == "param" || name == "throws" || name == "exception") {
                words(text, end, span.last + 1).firstOrNull()
            } else {
                null
            }
            out.add(Tag(name, start, end, value))
        }
        return out
    }

    /** The whitespace- or comma-separated words of [text] between [from] and [to]. */
    private fun words(text: String, from: Int, to: Int): List<Word> {
        val out = ArrayList<Word>()
        var i = from
        while (i < to) {
            while (i < to && (text[i].isWhitespace() || text[i] == ',')) i++
            val s = i
            while (i < to && !text[i].isWhitespace() && text[i] != ',') i++
            if (i > s) out.add(Word(text.substring(s, i), s, i))
        }
        return out
    }
}
