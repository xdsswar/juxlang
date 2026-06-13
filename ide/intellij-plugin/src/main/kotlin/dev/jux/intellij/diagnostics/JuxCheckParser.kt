package dev.jux.intellij.diagnostics

import com.google.gson.JsonParser

/**
 * One semantic diagnostic from `juxc --check --diagnostic-format json`, reduced
 * to what the editor annotator needs: the stable code, the severity, the
 * message, the owning file, and the location to underline.
 *
 * Both the 1-based line/column AND the UTF-8 byte range are carried. The editor
 * prefers line/column — it's line-ending-agnostic, so it maps correctly onto
 * IntelliJ's `\n`-normalized in-memory document even though juxc computed the
 * offsets from the on-disk bytes (which on Windows include `\r`). Byte offsets
 * are the fallback when line/column are absent.
 */
data class JuxDiagnostic(
    val code: String,
    val severity: String, // "error" | "warning" | "lint" | "note"
    val message: String,
    val file: String, // forward-slash path, exactly as juxc emitted it
    val byteStart: Int,
    val byteEnd: Int,
    val lineStart: Int = 0, // 1-based; 0 = absent
    val colStart: Int = 0, // 1-based char column
    val lineEnd: Int = 0,
    val colEnd: Int = 0,
)

/**
 * Parses the NDJSON stream from `juxc --check --diagnostic-format json`
 * (JUX-DIAGNOSTICS-ADDENDUM §D.2) and converts juxc's UTF-8 byte offsets into
 * Java string (UTF-16) offsets for the editor.
 *
 * Diagnostics without a `primary_span` (file-less, e.g. a cross-unit symbol
 * conflict) and the trailing `{"summary":…}` line are skipped — there's nothing
 * to underline. Parsing is line-by-line and exception-tolerant so a single
 * malformed line never sinks the rest.
 */
object JuxCheckParser {

    /** Parse the full stdout into a flat diagnostic list. */
    fun parse(stdout: String): List<JuxDiagnostic> {
        val out = ArrayList<JuxDiagnostic>()
        for (raw in stdout.lineSequence()) {
            val line = raw.trim()
            if (line.length < 2 || line[0] != '{') continue
            val obj = try {
                JsonParser.parseString(line).asJsonObject
            } catch (_: Throwable) {
                continue
            }
            // The summary line has no "code"; file-less diagnostics no span.
            if (!obj.has("code") || !obj.has("primary_span")) continue
            val span = try {
                obj.getAsJsonObject("primary_span")
            } catch (_: Throwable) {
                continue
            }
            try {
                out.add(
                    JuxDiagnostic(
                        code = obj.get("code").asString,
                        severity = obj.get("severity").asString,
                        message = obj.get("message").asString,
                        file = span.get("file").asString,
                        byteStart = span.get("byte_start").asInt,
                        byteEnd = span.get("byte_end").asInt,
                        lineStart = intOr(span, "line_start", 0),
                        colStart = intOr(span, "column_start", 0),
                        lineEnd = intOr(span, "line_end", 0),
                        colEnd = intOr(span, "column_end", 0),
                    ),
                )
            } catch (_: Throwable) {
                // A line missing an expected field is skipped, not fatal.
            }
        }
        return out
    }

    /** A JSON int field, or [default] when missing/non-numeric. */
    private fun intOr(obj: com.google.gson.JsonObject, name: String, default: Int): Int =
        try {
            if (obj.has(name)) obj.get(name).asInt else default
        } catch (_: Throwable) {
            default
        }

    /**
     * Map a UTF-8 [byteOffset] (what juxc reports) to the corresponding index
     * into [text] as a Java string (UTF-16 code units, what the editor uses).
     * juxc's span boundaries always fall on character boundaries, so this is
     * exact; for pure-ASCII source it's the identity. Clamps to the text end.
     */
    fun byteToCharOffset(text: String, byteOffset: Int): Int {
        if (byteOffset <= 0) return 0
        var bytes = 0
        var i = 0
        while (i < text.length && bytes < byteOffset) {
            val cp = text.codePointAt(i)
            bytes += utf8Len(cp)
            i += Character.charCount(cp)
        }
        return i
    }

    /** UTF-8 encoded length of a Unicode code point. */
    private fun utf8Len(cp: Int): Int = when {
        cp < 0x80 -> 1
        cp < 0x800 -> 2
        cp < 0x10000 -> 3
        else -> 4
    }
}
