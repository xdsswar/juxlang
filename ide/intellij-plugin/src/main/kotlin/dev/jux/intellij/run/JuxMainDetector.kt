package dev.jux.intellij.run

/**
 * Lightweight detection of a Jux entry point.
 *
 * Phase-1 detection is a text scan (no PSI): it looks for a top-level function
 * named `main` whose return type is `void` or `int`, optionally `public` and/or
 * `async`, matching the accepted entry-point shapes from
 * `JUX-ENTRY-POINTS-ADDENDUM.md` (the same set `juxc-tycheck` enforces). This
 * is good enough to decide whether a file is runnable; the compiler does the
 * authoritative signature check at build time.
 */
object JuxMainDetector {
    // ^(public)? (async)? (void|int) main ( ...
    // MULTILINE so `^` matches each line start; we don't try to skip comments
    // or strings in Phase 1 — a false positive only means an extra Run option.
    private val MAIN = Regex(
        """(?m)^\s*(public\s+|private\s+|protected\s+)?(async\s+)?(void|int)\s+main\s*\(""",
    )

    /** True if `text` appears to declare a runnable `main`. */
    fun hasMain(text: String): Boolean = MAIN.containsMatchIn(text)
}
