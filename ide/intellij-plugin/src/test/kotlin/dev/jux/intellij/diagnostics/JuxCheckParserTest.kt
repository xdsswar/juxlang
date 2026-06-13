package dev.jux.intellij.diagnostics

import junit.framework.TestCase

/**
 * [JuxCheckParser] — parsing the NDJSON from `juxc --check --diagnostic-format
 * json` and mapping juxc's UTF-8 byte offsets to UTF-16 string offsets. The
 * sample lines mirror the real schema (JUX-DIAGNOSTICS-ADDENDUM §D.2).
 */
class JuxCheckParserTest : TestCase() {

    fun testParsesDiagnosticAndSkipsSummary() {
        val ndjson = """
            {"code":"E0301","severity":"error","message":"cannot find `x` in this scope","primary_span":{"file":"C:/p/Bad.jux","byte_start":69,"byte_end":83,"line_start":4,"line_end":4,"column_start":17,"column_end":31,"snippet":"        int x = x;","highlight_start":16,"highlight_end":30},"docs_url":"https://docs.jux-lang.org/diag/E0301"}
            {"summary":{"errors":1,"warnings":0,"files_compiled":38,"duration_ms":0}}
        """.trimIndent()
        val diags = JuxCheckParser.parse(ndjson)
        assertEquals(1, diags.size)
        val d = diags[0]
        assertEquals("E0301", d.code)
        assertEquals("error", d.severity)
        assertEquals("cannot find `x` in this scope", d.message)
        assertEquals("C:/p/Bad.jux", d.file)
        assertEquals(69, d.byteStart)
        assertEquals(83, d.byteEnd)
        // Line/column are carried too — the editor prefers these (CRLF-safe).
        assertEquals(4, d.lineStart)
        assertEquals(17, d.colStart)
        assertEquals(4, d.lineEnd)
        assertEquals(31, d.colEnd)
    }

    fun testLineColDefaultToZeroWhenAbsent() {
        val ndjson =
            """{"code":"E0400","severity":"warning","message":"ok","primary_span":{"file":"a.jux","byte_start":1,"byte_end":2}}"""
        val d = JuxCheckParser.parse(ndjson).single()
        assertEquals(0, d.lineStart) // absent -> 0 -> annotator falls back to byte mapping
        assertEquals(1, d.byteStart)
    }

    fun testSkipsFilelessAndMalformedLines() {
        val ndjson = """
            not json at all
            {"code":"E9999","severity":"error","message":"no span here"}
            {"code":"E0400","severity":"warning","message":"ok","primary_span":{"file":"a.jux","byte_start":1,"byte_end":2}}
        """.trimIndent()
        val diags = JuxCheckParser.parse(ndjson)
        // Only the well-formed, spanned diagnostic survives.
        assertEquals(1, diags.size)
        assertEquals("E0400", diags[0].code)
        assertEquals("warning", diags[0].severity)
    }

    fun testByteToCharOffsetIsIdentityForAscii() {
        val text = "package demo;\nint x = y;"
        assertEquals(0, JuxCheckParser.byteToCharOffset(text, 0))
        assertEquals(14, JuxCheckParser.byteToCharOffset(text, 14))
        assertEquals(text.length, JuxCheckParser.byteToCharOffset(text, 999))
    }

    fun testByteToCharOffsetHandlesMultibyte() {
        // "é" is 2 UTF-8 bytes but 1 Java char; "𝄞" (U+1D11E) is 4 bytes, 2 chars.
        val text = "é𝄞z" // chars: é(1) 𝄞(2 surrogates) z(1) = 4 chars; bytes: 2+4+1 = 7
        // byte offset 2 = just past "é" → char index 1
        assertEquals(1, JuxCheckParser.byteToCharOffset(text, 2))
        // byte offset 6 = just past "é𝄞" → char index 3 (é=1 + surrogate pair=2)
        assertEquals(3, JuxCheckParser.byteToCharOffset(text, 6))
        // byte offset 7 = end → char index 4
        assertEquals(4, JuxCheckParser.byteToCharOffset(text, 7))
    }
}
