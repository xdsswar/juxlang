package dev.jux.intellij.run

/**
 * Lightweight detection of a Jux entry point.
 *
 * Phase-1 detection is a text scan (no PSI): it looks for a function named
 * `main` whose return type is `void` or `int`, preceded by any number of
 * modifiers in any order (`public`, `static`, `async`, …). This matches both a
 * free `main` (no class) and a `static main` inside a class, so the Run option
 * shows up for either. A false positive only means an extra Run option; the
 * compiler does the authoritative signature check at build time.
 */
object JuxMainDetector {
    // ^  [modifiers...]  (void|int)  main  (
    // Modifiers are an unordered, possibly-empty run of known keywords — this
    // is what makes `static void main`, `public static void main`, and a bare
    // `void main` all match. MULTILINE so `^` matches each line start.
    private val MAIN = Regex(
        """(?m)^\s*(?:(?:public|private|protected|internal|static|final|abstract|async|native)\s+)*(?:void|int)\s+main\s*\(""",
    )

    /** True if `text` appears to declare a runnable `main`. */
    fun hasMain(text: String): Boolean = MAIN.containsMatchIn(text)
}
