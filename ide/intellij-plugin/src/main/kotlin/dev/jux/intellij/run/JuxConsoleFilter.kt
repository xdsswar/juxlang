package dev.jux.intellij.run

import com.intellij.execution.filters.Filter
import com.intellij.execution.filters.OpenFileHyperlinkInfo
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.LocalFileSystem

/**
 * Turns `path:line:col` locations in the run / test console into clickable
 * hyperlinks that jump to the exact spot — the way Java's (and cargo's)
 * compiler output behaves. The Jux toolchain prints diagnostics as
 * `path:line:col: [Code] severity: message` (juxc) and the test runner echoes
 * the same shape, so one matcher covers both consoles.
 */
class JuxConsoleFilter(private val project: Project) : Filter {

    override fun applyFilter(line: String, entireLength: Int): Filter.Result? {
        val m = matchDiagnostic(line) ?: return null
        // LocalFileSystem wants forward slashes; juxc may print either.
        val vf = LocalFileSystem.getInstance().findFileByPath(m.path.replace('\\', '/')) ?: return null
        val lineStart = entireLength - line.length
        val link = OpenFileHyperlinkInfo(
            project,
            vf,
            (m.line - 1).coerceAtLeast(0), // editor lines are 0-based
            (m.column - 1).coerceAtLeast(0), // editor columns are 0-based
        )
        return Filter.Result(lineStart + m.start, lineStart + m.end, link)
    }

    companion object {
        /**
         * `path:line:col` where the location sits at the start of the line
         * (juxc's compact form, possibly indented) or right after a Rust-style
         * `-->` pointer. The path may carry one leading drive letter (`C:`) but
         * no other colon, ends in `.jux`, and may contain spaces or either
         * slash. Anchoring to the line start (rather than scanning mid-line)
         * avoids swallowing a preceding word into the path.
         */
        private val PATTERN = Regex("""^\s*(?:-->\s*)?((?:[A-Za-z]:)?[^:\n\r]*\.jux):(\d+):(\d+)""")

        /** A located diagnostic: file path, 1-based line/column, and the
         *  range within the scanned text covering exactly `path:line:col`. */
        data class Match(val path: String, val line: Int, val column: Int, val start: Int, val end: Int)

        /** First `path:line:col` location in [text], or null when none. */
        fun matchDiagnostic(text: String): Match? {
            val mr = PATTERN.find(text) ?: return null
            val pathGroup = mr.groups[1] ?: return null
            val colGroup = mr.groups[3] ?: return null
            val path = pathGroup.value.trim()
            if (path.isEmpty()) return null
            val line = mr.groupValues[2].toIntOrNull() ?: return null
            val col = mr.groupValues[3].toIntOrNull() ?: return null
            // Link range spans path…col only (excludes any indent / `-->`).
            return Match(path, line, col, pathGroup.range.first, colGroup.range.last + 1)
        }
    }
}
