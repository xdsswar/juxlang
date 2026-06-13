package dev.jux.intellij.run

import junit.framework.TestCase

/**
 * [JuxConsoleFilter.matchDiagnostic] — recognizing `path:line:col` locations in
 * console output across path shapes, so the run/test console can link them.
 */
class JuxConsoleFilterTest : TestCase() {

    fun testWindowsForwardSlashPath() {
        val line = "C:/Users/x/Bad.jux:4:17: [E0301] error: cannot find `x`"
        val m = JuxConsoleFilter.matchDiagnostic(line)!!
        assertEquals("C:/Users/x/Bad.jux", m.path)
        assertEquals(4, m.line)
        assertEquals(17, m.column)
        // The hyperlink range covers exactly `path:line:col`.
        assertEquals("C:/Users/x/Bad.jux:4:17", line.substring(m.start, m.end))
    }

    fun testWindowsBackslashPathWithSpaces() {
        val line = """C:\Users\My App\Bad.jux:10:3: [E0400] warning: hmm"""
        val m = JuxConsoleFilter.matchDiagnostic(line)!!
        assertEquals("""C:\Users\My App\Bad.jux""", m.path)
        assertEquals(10, m.line)
        assertEquals(3, m.column)
    }

    fun testUnixPath() {
        val m = JuxConsoleFilter.matchDiagnostic("/home/u/proj/Main.jux:1:1: [E0200] error: oops")!!
        assertEquals("/home/u/proj/Main.jux", m.path)
        assertEquals(1, m.line)
        assertEquals(1, m.column)
    }

    fun testRustStyleArrowPointer() {
        // A Rust-style `--> path:line:col` secondary-location line (indented).
        val line = "   --> /a/b/Util.jux:8:5"
        val m = JuxConsoleFilter.matchDiagnostic(line)!!
        assertEquals("/a/b/Util.jux", m.path)
        assertEquals(8, m.line)
        assertEquals(5, m.column)
        // The link excludes the indent and the `--> ` pointer.
        assertEquals("/a/b/Util.jux:8:5", line.substring(m.start, m.end))
    }

    fun testIndentedLocationLinksPathOnly() {
        val line = "    C:/p/Bad.jux:2:9: [E0301] error: x"
        val m = JuxConsoleFilter.matchDiagnostic(line)!!
        assertEquals("C:/p/Bad.jux:2:9", line.substring(m.start, m.end))
    }

    fun testNoLocationReturnsNull() {
        assertNull(JuxConsoleFilter.matchDiagnostic("juxc: 0 diagnostics, lowering complete"))
        assertNull(JuxConsoleFilter.matchDiagnostic("Hello, world!"))
    }
}
