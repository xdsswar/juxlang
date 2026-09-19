package dev.jux.intellij.format

import com.intellij.psi.PsiFile
import com.intellij.application.options.CodeStyle
import com.intellij.psi.codeStyle.CodeStyleSettings
import com.intellij.psi.codeStyle.CommonCodeStyleSettings
import com.intellij.psi.codeStyle.CustomCodeStyleSettings

/**
 * Jux's own code style settings: the Imports tab (Settings | Editor | Code
 * Style | Jux | Imports), Java's "Imports" page for Jux, and the brace
 * placement of the Wrapping and Braces tab.
 *
 * Public fields, because the platform serializes a [CustomCodeStyleSettings]
 * by reflecting over them, and a project that shares its code style in
 * `.idea/codeStyles` carries these along.
 */
@Suppress("PropertyName")
class JuxCodeStyleSettings(container: CodeStyleSettings) : CustomCodeStyleSettings("JuxCodeStyleSettings", container) {

    /**
     * The import layout: groups in order, separated by `;`, each group a
     * `|`-separated list of package prefixes, and `*` for every import no
     * other group takes. The default is the corpus's order: bound crates, the
     * Jux library, the project.
     */
    @JvmField
    var IMPORT_LAYOUT: String = DEFAULT_LAYOUT

    /** A blank line between two import groups. */
    @JvmField
    var BLANK_LINE_BETWEEN_IMPORT_GROUPS: Boolean = true

    /**
     * Java's "class count to use import with '*'": once this many names are
     * imported one by one from a package, Optimize Imports and auto-import use
     * `import pkg.*;` instead. `0` means never, the default: Jux's grouped
     * import (`import pkg.{A, B};`) already keeps a long list on one line.
     */
    @JvmField
    var NAMES_COUNT_TO_USE_WILDCARD: Int = 0

    // ---- Wrapping and Braces | Braces placement ------------------------------
    //
    // Java keeps these in the common settings, with five choices; Jux keeps
    // its own with the three its formatter implements (see [BRACE_STYLES]),
    // under Java's names.

    /** Where a type body's `{` goes: class, interface, enum, record. */
    @JvmField
    var CLASS_BRACE_STYLE: Int = END_OF_LINE

    /** Where a method's, constructor's or operator's body `{` goes. */
    @JvmField
    var METHOD_BRACE_STYLE: Int = END_OF_LINE

    /** Where a block-bodied lambda's `{` goes. */
    @JvmField
    var LAMBDA_BRACE_STYLE: Int = END_OF_LINE

    /** Where every other `{` goes: `if`, loops, `try`, `switch`, labeled blocks. */
    @JvmField
    var BRACE_STYLE: Int = END_OF_LINE

    /** The layout as groups of prefixes; `*` is the catch-all group. */
    fun layoutGroups(): List<List<String>> = parseLayout(IMPORT_LAYOUT)

    companion object {
        const val DEFAULT_LAYOUT = "rust.|c.|cpp.;jux.;*"

        /** Java's values for the brace styles Jux implements (CommonCodeStyleSettings). */
        const val END_OF_LINE = CommonCodeStyleSettings.END_OF_LINE
        const val NEXT_LINE = CommonCodeStyleSettings.NEXT_LINE
        const val NEXT_LINE_IF_WRAPPED = CommonCodeStyleSettings.NEXT_LINE_IF_WRAPPED

        /** The brace styles, as the Wrapping and Braces tab names them, and their values. */
        val BRACE_STYLE_NAMES = arrayOf("End of line", "Next line", "Next line if wrapped")
        val BRACE_STYLES = intArrayOf(END_OF_LINE, NEXT_LINE, NEXT_LINE_IF_WRAPPED)

        /** The Jux code style in effect for [file]. */
        fun of(file: PsiFile): JuxCodeStyleSettings = CodeStyle.getCustomSettings(file, JuxCodeStyleSettings::class.java)

        /** `"a.|b.;*"` as `[[a., b.], [*]]`, a catch-all added when missing. */
        fun parseLayout(layout: String): List<List<String>> {
            val groups = layout.split(';').map { g -> g.split('|').map { it.trim() }.filter { it.isNotEmpty() } }.filter { it.isNotEmpty() }
            return if (groups.any { "*" in it }) groups else groups + listOf(listOf("*"))
        }

        /** The inverse of [parseLayout]. */
        fun formatLayout(groups: List<List<String>>): String = groups.joinToString(";") { it.joinToString("|") }

        /**
         * Which group of [layout] an import path belongs to: the group with
         * the longest matching prefix, else the catch-all.
         */
        fun groupIndex(layout: List<List<String>>, path: String): Int {
            var best = -1
            var bestLength = -1
            layout.forEachIndexed { i, group ->
                for (prefix in group) {
                    if (prefix != "*" && path.startsWith(prefix) && prefix.length > bestLength) {
                        best = i
                        bestLength = prefix.length
                    }
                }
            }
            return if (best >= 0) best else layout.indexOfFirst { "*" in it }.coerceAtLeast(0)
        }
    }
}
