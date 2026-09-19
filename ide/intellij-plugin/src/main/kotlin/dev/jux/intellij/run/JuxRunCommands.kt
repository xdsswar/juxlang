package dev.jux.intellij.run

/**
 * The argument lists a Jux run configuration hands to `jux` / `juxc`. Kept
 * pure (strings in, strings out) so every combination of mode, profile and
 * example unit-tests without starting a process.
 *
 * Every console command asks for the framed `human` diagnostic format with
 * color forced on (JUX-DIAGNOSTICS-ADDENDUM §D.1.6): a run console is not a
 * terminal, so the tools would otherwise fall back to the one-line format.
 * The console interprets the ANSI colors, and [JuxConsoleFilter] still makes
 * every `path:line:col` in the frame clickable. The editor's own background
 * check keeps using `--diagnostic-format json` and is not built here.
 */
object JuxRunCommands {

    /** The framed diagnostic format, colored, for a run console (§D.1.6). */
    val CONSOLE_DIAGNOSTICS: List<String> = listOf("--diagnostic-format", "human", "--color", "always")

    /** Marker stored in the example field meaning "build every example" (`--examples`). */
    const val ALL_EXAMPLES = "<all>"

    /**
     * `jux run` for a project: the main program, one example
     * (`--example <name>`), or, for [ALL_EXAMPLES], `jux build --examples`
     * (every example is its own program, so building them all is what the
     * build system offers; there is no single one to run). A non-blank
     * [profile] adds `--profile <name>` (§B.9).
     */
    fun projectRun(example: String, profile: String): List<String> = buildList {
        val ex = example.trim()
        if (ex == ALL_EXAMPLES) {
            add("build")
            add("--examples")
        } else {
            add("run")
            if (ex.isNotEmpty()) {
                add("--example")
                add(ex)
            }
        }
        profileArgs(profile)?.let(::addAll)
        addAll(CONSOLE_DIAGNOSTICS)
    }

    /**
     * `jux test [pattern]` (§TS.8), or `jux test --doc` for the doc-comment
     * examples (§12.5). `--profile` and `--release` are mutually exclusive in
     * the CLI, so a chosen profile wins over the release checkbox.
     */
    fun test(pattern: String, release: Boolean, profile: String, doc: Boolean): List<String> = buildList {
        add("test")
        if (doc) {
            add("--doc")
        } else {
            pattern.trim().takeIf { it.isNotEmpty() }?.let(::add)
        }
        val profiled = profileArgs(profile)
        if (profiled != null) addAll(profiled) else if (release) add("--release")
        addAll(CONSOLE_DIAGNOSTICS)
    }

    /** `juxc <target> --run` for a file with no `jux.toml` above it. */
    fun standalone(target: String): List<String> = listOf(target, "--run") + CONSOLE_DIAGNOSTICS

    private fun profileArgs(profile: String): List<String>? =
        profile.trim().takeIf { it.isNotEmpty() }?.let { listOf("--profile", it) }
}
