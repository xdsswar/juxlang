package dev.jux.intellij.run

import dev.jux.intellij.run.JuxTestOutputParser.Line
import junit.framework.TestCase

/**
 * The doc-example report of `jux test --doc` (bin/jux `run_doctests_for`),
 * classified line by line for the test console: header, `PASS Owner
 * (file:line)`, `FAIL Owner (file:line)` with its indented reason, summary.
 */
class JuxDocTestOutputTest : TestCase() {

    fun testHeader() {
        assertEquals(Line.DocRunStart(3, "geo.shapes"), JuxTestOutputParser.classifyDoc("running 3 doc example(s) of `geo.shapes`"))
    }

    fun testPassAndFailCarryTheItemsLocation() {
        assertEquals(
            Line.DocPass("Shape.area", "src/geo/Shape.jux:12"),
            JuxTestOutputParser.classifyDoc("  PASS Shape.area (src/geo/Shape.jux:12)"),
        )
        assertEquals(
            Line.DocFail("greet", "C:\\proj\\src\\lib.jux:4"),
            JuxTestOutputParser.classifyDoc("  FAIL greet (C:\\proj\\src\\lib.jux:4)\r"),
        )
    }

    fun testSummary() {
        assertEquals(Line.DocSummary(ok = true, passed = 3, failed = 0), JuxTestOutputParser.classifyDoc("doc examples: ok. 3 passed; 0 failed"))
        assertEquals(Line.DocSummary(ok = false, passed = 2, failed = 1), JuxTestOutputParser.classifyDoc("doc examples: FAILED. 2 passed; 1 failed"))
    }

    fun testReasonLinesAndUnitTestLinesStillClassify() {
        assertEquals(Line.Other, JuxTestOutputParser.classifyDoc("       error: E0413 no method `nosuch`"))
        // The unit-test forms keep working through the doc classifier.
        assertEquals(Line.Pass("pkg.works"), JuxTestOutputParser.classifyDoc("  PASS pkg.works"))
        assertEquals(Line.Fail("pkg.breaks", "assertEqual: expected `1`"), JuxTestOutputParser.classifyDoc("  FAIL pkg.breaks: assertEqual: expected `1`"))
    }

    fun testLocationSplitsAtTheLastColon() {
        assertEquals("C:\\proj\\A.jux" to 7, JuxTestOutputParser.splitLocation("C:\\proj\\A.jux:7"))
        assertEquals("src/a.jux" to 12, JuxTestOutputParser.splitLocation("src/a.jux:12"))
        assertNull(JuxTestOutputParser.splitLocation("no-line"))
        assertNull(JuxTestOutputParser.splitLocation("a.jux:x"))
    }
}
