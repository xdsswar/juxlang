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

        /** Anything else (compiler output, program prints, blank lines). */
        data object Other : Line
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
