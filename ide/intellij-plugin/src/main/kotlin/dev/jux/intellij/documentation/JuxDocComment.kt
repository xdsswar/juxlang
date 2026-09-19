package dev.jux.intellij.documentation

import com.intellij.openapi.util.text.StringUtil

/**
 * A doc comment split the way Javadoc splits one: the description, then the
 * block tags `@param name text`, `@return text`, `@throws Type text` (and
 * its synonym `@exception`), `@since text` and `@see ref`. A tag's text runs
 * until the next tag. Unknown tags are left in the description.
 */
data class JuxDocComment(
    val description: String,
    val params: List<Pair<String, String>>,
    val returns: String?,
    val throws: List<Pair<String, String>>,
    val since: String?,
    val see: List<String>,
) {
    companion object {
        private val TAG = Regex("""^@(param|return|returns|throws|exception|since|see)\b\s*(.*)$""", RegexOption.DOT_MATCHES_ALL)

        /** Split the cleaned text of a doc comment (markers already stripped). */
        fun parse(text: String): JuxDocComment {
            val description = StringBuilder()
            val params = ArrayList<Pair<String, String>>()
            val throws = ArrayList<Pair<String, String>>()
            val see = ArrayList<String>()
            var returns: String? = null
            var since: String? = null
            // Group lines into the description and one chunk per block tag.
            val chunks = ArrayList<String>()
            for (line in text.lines()) {
                if (line.trimStart().startsWith("@") && TAG.matches(line.trim())) chunks.add(line.trim())
                else if (chunks.isEmpty()) description.append(line).append('\n')
                else chunks[chunks.size - 1] = chunks.last() + "\n" + line
            }
            for (chunk in chunks) {
                val m = TAG.matchEntire(chunk) ?: continue
                val body = m.groupValues[2].trim()
                when (m.groupValues[1]) {
                    "param" -> params.add(splitFirstWord(body))
                    "return", "returns" -> returns = body
                    "throws", "exception" -> throws.add(splitFirstWord(body))
                    "since" -> since = body
                    "see" -> see.add(body)
                }
            }
            return JuxDocComment(description.toString().trim(), params, returns, throws, since, see)
        }

        /** `name the rest` as (`name`, `the rest`); `<T> text` keeps its brackets. */
        private fun splitFirstWord(body: String): Pair<String, String> {
            val word = body.takeWhile { !it.isWhitespace() }
            return StringUtil.escapeXmlEntities(word) to body.drop(word.length).trim()
        }

        private val INLINE_CODE = Regex("""\{@(code|literal)\s+([^}]*)}""")
        private val INLINE_LINK = Regex("""\{@(link|linkplain)\s+([^}\s]*)(?:\s+([^}]*))?}""")
        private val BACKTICKS = Regex("`([^`]+)`")

        /**
         * Doc text as HTML: escaped, `{@code x}` and Markdown-style `` `x` ``
         * as code, `{@link Type#member label}` as its label (or target) in
         * code, and a blank line as a paragraph break.
         */
        fun toHtml(text: String): String {
            var html = StringUtil.escapeXmlEntities(text)
            html = INLINE_CODE.replace(html) { "<code>${it.groupValues[2]}</code>" }
            html = INLINE_LINK.replace(html) { m ->
                val label = m.groupValues[3].ifBlank { m.groupValues[2].replace('#', '.') }
                "<code>$label</code>"
            }
            html = BACKTICKS.replace(html) { "<code>${it.groupValues[1]}</code>" }
            return html.split(Regex("\n\\s*\n")).joinToString("</p><p>", "<p>", "</p>") { it.trim().replace("\n", " ") }
        }
    }
}
