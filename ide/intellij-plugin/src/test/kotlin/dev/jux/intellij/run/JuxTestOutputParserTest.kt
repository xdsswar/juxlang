package dev.jux.intellij.run

import dev.jux.intellij.run.JuxTestOutputParser.Line
import junit.framework.TestCase

/**
 * The pure §TS.7 line classifier — every shape the generated runner prints,
 * including the hook-failure variants and the filtered summary. No fixture
 * needed; this is exactly why the parser is IDE-free.
 */
class JuxTestOutputParserTest : TestCase() {

    fun testRunStart() {
        assertEquals(Line.RunStart(12), JuxTestOutputParser.classify("running 12 tests"))
        assertEquals(Line.RunStart(1), JuxTestOutputParser.classify("running 1 test"))
    }

    fun testPass() {
        assertEquals(
            Line.Pass("demo.testing.numbersAddUp"),
            JuxTestOutputParser.classify("  PASS demo.testing.numbersAddUp"),
        )
        // No package — bare function name.
        assertEquals(Line.Pass("solo"), JuxTestOutputParser.classify("  PASS solo"))
    }

    fun testFailSplitsAtFirstColonOnly() {
        val line = "  FAIL demo.other: assertEqual: expected `5`, got `4`"
        assertEquals(
            Line.Fail("demo.other", "assertEqual: expected `5`, got `4`"),
            JuxTestOutputParser.classify(line),
        )
    }

    fun testHookFailureNames() {
        assertEquals(
            Line.Fail("demo.t", "<afterEach> boom"),
            JuxTestOutputParser.classify("  FAIL demo.t: <afterEach> boom"),
        )
        // Standalone afterAll failure has no test name.
        assertEquals(
            Line.Fail("<afterAll>", "tear-down exploded"),
            JuxTestOutputParser.classify("  FAIL <afterAll>: tear-down exploded"),
        )
    }

    fun testSummaries() {
        assertEquals(
            Line.Summary(ok = true, passed = 3, failed = 0, filtered = 0),
            JuxTestOutputParser.classify("test result: ok. 3 passed; 0 failed"),
        )
        assertEquals(
            Line.Summary(ok = false, passed = 2, failed = 1, filtered = 0),
            JuxTestOutputParser.classify("test result: FAILED. 2 passed; 1 failed"),
        )
        assertEquals(
            Line.Summary(ok = true, passed = 1, failed = 0, filtered = 4),
            JuxTestOutputParser.classify("test result: ok. 1 passed; 0 failed; 4 filtered out"),
        )
    }

    fun testWindowsLineEndingsTolerated() {
        assertEquals(Line.RunStart(2), JuxTestOutputParser.classify("running 2 tests\r\n"))
        assertEquals(Line.Pass("a.b"), JuxTestOutputParser.classify("  PASS a.b\r"))
    }

    fun testEverythingElseIsOther() {
        assertEquals(Line.Other, JuxTestOutputParser.classify(""))
        assertEquals(Line.Other, JuxTestOutputParser.classify("Compiling demo v0.1.0"))
        assertEquals(Line.Other, JuxTestOutputParser.classify("some program output"))
        // Program output that merely contains PASS mid-line must not match.
        assertEquals(Line.Other, JuxTestOutputParser.classify("PASS without indent"))
    }
}
