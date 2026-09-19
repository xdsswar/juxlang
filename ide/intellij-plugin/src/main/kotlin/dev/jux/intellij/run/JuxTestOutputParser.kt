package dev.jux.intellij.run

/**
 * Pure line classifier for the `jux test` runner output (§TS.7) — kept free of
 * any IntelliJ dependency so it unit-tests without a fixture. The exact shapes
 * (from the generated runner, `juxc-backend-rust/src/lib.rs`):
 *
 * ```text
 * running N tests
 *   PASS pkg.testName
 *   FAIL pkg.other: assertEqual: expected `5`, got `4`
 *   FAIL <afterAll>: boom
 *
 * test result: FAILED. M passed; K failed; J filtered out
 * ```
 *
 * `test result: ok.` when everything passed; the `; J filtered out` suffix
 * appears only under a `jux test <pattern>` filter (§TS.8). A FAIL message may
 * itself contain `:` and backticks — only the FIRST `: ` after the name splits.
 */
object JuxTestOutputParser {

    /** One classified runner-output line. */
    sealed interface Line {
        /** `running N tests` — the run header; [count] drives the progress bar. */
        data class RunStart(val count: Int) : Line

        /** `  PASS <name>` — a passed test, [name] is its display name. */
        data class Pass(val name: String) : Line

        /** `  FAIL <name>: <message>` — a failed test (or `<afterAll>` hook). */
        data class Fail(val name: String, val message: String) : Line

        /** The `test result: …` summary line. */
        data class Summary(val ok: Boolean, val passed: Int, val failed: Int, val filtered: Int) : Line

        /** `running N doc example(s) of `pkg`` (`jux test --doc`, §12.5). */
        data class DocRunStart(val count: Int, val pkg: String) : Line

        /** `  PASS Owner (file:line)`: a doc example of [owner] that compiled and ran. */
        data class DocPass(val owner: String, val location: String) : Line

        /**
         * `  FAIL Owner (file:line)`: a doc example that failed. The reason
         * follows on lines indented seven spaces, classified as [Other].
         */
        data class DocFail(val owner: String, val location: String) : Line

        /** `doc examples: ok. 3 passed; 0 failed` (or `FAILED.`). */
        data class DocSummary(val ok: Boolean, val passed: Int, val failed: Int) : Line

        /** Anything else (compiler output, program prints, blank lines). */
        data object Other : Line
    }

    // The doc-example report of `jux test --doc` / `jux doc` (bin/jux,
    // `run_doctests_for`): the owner is the documented item (`Shape.area`),
    // the location its declaration (`src/geo/Shape.jux:12`, possibly a
    // Windows path with a drive colon).
    private val DOC_RUN_START = Regex("""^running (\d+) doc example\(s\) of `([^`]*)`$""")
    private val DOC_PASS = Regex("""^\s+PASS (\S+) \((.+:\d+)\)$""")
    private val DOC_FAIL = Regex("""^\s+FAIL (\S+) \((.+:\d+)\)$""")
    private val DOC_SUMMARY = Regex("""^doc examples: (ok|FAILED)\. (\d+) passed; (\d+) failed$""")

    /**
     * Classify one `jux test --doc` line: the doc forms first (their PASS/FAIL
     * lines end in a `(file:line)` location the unit-test forms never carry),
     * then the ordinary test forms, so one console can read both reports.
     */
    fun classifyDoc(line: String): Line {
        val t = line.trimEnd('\r', '\n')
        DOC_RUN_START.matchEntire(t)?.let { return Line.DocRunStart(it.groupValues[1].toInt(), it.groupValues[2]) }
        DOC_PASS.matchEntire(t)?.let { return Line.DocPass(it.groupValues[1], it.groupValues[2]) }
        DOC_FAIL.matchEntire(t)?.let { return Line.DocFail(it.groupValues[1], it.groupValues[2]) }
        DOC_SUMMARY.matchEntire(t)?.let {
            return Line.DocSummary(it.groupValues[1] == "ok", it.groupValues[2].toInt(), it.groupValues[3].toInt())
        }
        return classify(t)
    }

    /**
     * Split a doc example's `file:line` location into its path and 1-based
     * line. The LAST colon splits, so `C:\proj\src\A.jux:7` keeps its drive.
     */
    fun splitLocation(location: String): Pair<String, Int>? {
        val at = location.lastIndexOf(':')
        if (at <= 0) return null
        val line = location.substring(at + 1).toIntOrNull() ?: return null
        return location.substring(0, at) to line
    }

    // `running 12 tests` (also tolerates `running 1 test`).
    private val RUN_START = Regex("""^running (\d+) tests?$""")

    // Indented status lines. The name is everything up to the first `: ` for
    // FAIL (messages may contain further colons); PASS has no message.
    private val PASS = Regex("""^\s+PASS (\S.*)$""")
    private val FAIL = Regex("""^\s+FAIL ([^:]+): (.*)$""")

    // `test result: ok. 3 passed; 0 failed` / `test result: FAILED. 2 passed;
    // 1 failed; 4 filtered out`.
    private val SUMMARY = Regex(
        """^test result: (ok\.|FAILED\.) (\d+) passed; (\d+) failed(?:; (\d+) filtered out)?$""",
    )

    /** Classify one runner-output line ([line] may carry a trailing `\r`). */
    fun classify(line: String): Line {
        val t = line.trimEnd('\r', '\n')
        RUN_START.matchEntire(t)?.let { return Line.RunStart(it.groupValues[1].toInt()) }
        SUMMARY.matchEntire(t)?.let {
            return Line.Summary(
                ok = it.groupValues[1] == "ok.",
                passed = it.groupValues[2].toInt(),
                failed = it.groupValues[3].toInt(),
                filtered = it.groupValues[4].takeIf { g -> g.isNotEmpty() }?.toInt() ?: 0,
            )
        }
        FAIL.matchEntire(t)?.let {
            return Line.Fail(it.groupValues[1].trim(), it.groupValues[2])
        }
        PASS.matchEntire(t)?.let { return Line.Pass(it.groupValues[1].trim()) }
        return Line.Other
    }
}
