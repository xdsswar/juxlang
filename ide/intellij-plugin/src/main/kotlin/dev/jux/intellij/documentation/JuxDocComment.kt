package dev.jux.intellij.documentation

import com.intellij.openapi.util.text.StringUtil

/**
 * A doc comment split the way `jux doc` splits one (juxc-driver docgen
 * `parse_doc_comment`): the description, then the block tags `@param name
 * text`, `@return text` (or `@returns`), `@throws Type text` (or
 * `@exception`), `@deprecated reason`, `@since text` and `@see ref`. A tag's
 * text runs until the next tag or a blank line. Unknown tags stay in the
 * description, and nothing inside a ```` ``` ```` fenced block is a tag: an
 * example's `@annotation` is code.
 */
data class JuxDocComment(
    val description: String,
    val params: List<Pair<String, String>>,
    val returns: String?,
    val throws: List<Pair<String, String>>,
    val since: String?,
    val see: List<String>,
    val deprecated: String? = null,
) {
    companion object {
        private val TAG = Regex("""^@(param|return|returns|throws|exception|deprecated|since|see)\b\s*(.*)$""", RegexOption.DOT_MATCHES_ALL)

        /** Split the cleaned text of a doc comment (markers already stripped). */
        fun parse(text: String): JuxDocComment {
            val description = StringBuilder()
            val params = ArrayList<Pair<String, String>>()
            val throws = ArrayList<Pair<String, String>>()
            val see = ArrayList<String>()
            var returns: String? = null
            var since: String? = null
            var deprecated: String? = null
            // Group lines into the description and one chunk per block tag. A
            // tag's chunk ends at a blank line; fenced blocks are never tags.
            val chunks = ArrayList<String>()
            var open = false
            var inFence = false
            for (line in text.lines()) {
                val trimmed = line.trim()
                if (trimmed.startsWith("```")) {
                    inFence = !inFence
                    open = false
                    description.append(line).append('\n')
                    continue
                }
                if (!inFence && trimmed.startsWith("@") && TAG.matches(trimmed)) {
                    chunks.add(trimmed)
                    open = true
                } else if (open && !inFence && trimmed.isNotEmpty()) {
                    chunks[chunks.size - 1] = chunks.last() + "\n" + line
                } else {
                    open = false
                    description.append(line).append('\n')
                }
            }
            for (chunk in chunks) {
                val m = TAG.matchEntire(chunk) ?: continue
                val body = m.groupValues[2].trim()
                when (m.groupValues[1]) {
                    "param" -> params.add(splitFirstWord(body))
                    "return", "returns" -> returns = body
                    "throws", "exception" -> throws.add(splitFirstWord(body))
                    "deprecated" -> deprecated = body
                    "since" -> since = body
                    "see" -> see.add(body)
                }
            }
            return JuxDocComment(description.toString().trim('\n', ' '), params, returns, throws, since, see, deprecated)
        }

        /** `name the rest` as (`name`, `the rest`); `<T> text` keeps its brackets. */
        private fun splitFirstWord(body: String): Pair<String, String> {
            val word = body.takeWhile { !it.isWhitespace() }
            return StringUtil.escapeXmlEntities(word) to body.drop(word.length).trim()
        }

        /**
         * Doc text as HTML, the Markdown subset `jux doc` renders
         * ([JuxDocMarkdown]); ```` ```jux ```` blocks through [highlight].
         */
        fun toHtml(text: String, highlight: JuxDocMarkdown.Highlighter = JuxDocMarkdown.PLAIN): String =
            JuxDocMarkdown.toHtml(text, highlight)
    }
}
