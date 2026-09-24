package dev.jux.intellij

import java.io.File

/**
 * Every `.jux` file in the repository's `examples/` tree, for the corpus
 * tests to run their whole surface over.
 *
 * The five corpus tests each used to call `listFiles` on `examples/` itself,
 * which is NOT recursive: it sees the 272 single-file examples and none of
 * the 67 files inside the multi-file projects. That is the wrong 67. A
 * single-file example is a snippet; `metrics_console`, `gui_workspace` and
 * `paint_app` are the largest, newest, most feature-dense Jux in the repo --
 * generic classes across package boundaries, interfaces with default methods,
 * enums with bodies, tuple destructuring, `rust.<crate>` imports -- and the
 * plugin was never once asked to highlight, parse, complete or format any of
 * it.
 *
 * So this walks. Directories that hold build output rather than source are
 * skipped: they contain generated Rust and stub `.jux.d` files, and on a
 * developer machine they are large.
 */
object JuxCorpus {

    /** Directory names that never contain hand-written Jux. */
    private val SKIP = setOf("target", ".rust-build", ".jux-stubs", "build", ".git")

    /**
     * A corpus entry: the file, plus the name to configure it under.
     *
     * The two differ only where they must. A top-level example keeps its bare
     * name, because several of them deliberately declare the same type name
     * (`Tagged` is an interface in one and a class in two others) and
     * resolving a name in its own file first is exactly what the corpus
     * highlighting test exists to prove. A nested file takes a name derived
     * from its path, because `main.jux` appears in five projects and
     * `configureByText` would otherwise put them in one file.
     */
    data class Entry(val name: String, val file: File)

    /**
     * Every `.jux` under [root], sorted for a stable report order.
     *
     * Returns an empty list when `root` is not a directory, so a test can
     * decide for itself whether that is a skip or a failure.
     */
    fun entries(root: File): List<Entry> {
        if (!root.isDirectory) return emptyList()

        val out = mutableListOf<Entry>()
        val seen = mutableSetOf<String>()

        root.walkTopDown()
            .onEnter { dir -> dir == root || dir.name !in SKIP }
            .filter { it.isFile && it.name.endsWith(".jux") }
            .sortedBy { it.relativeTo(root).invariantSeparatorsPath }
            .forEach { file ->
                val relative = file.relativeTo(root).invariantSeparatorsPath
                val name = if (!relative.contains('/')) {
                    relative
                } else {
                    // `metrics_console/app/src/main.jux` -> `metrics_console_app_src_main.jux`
                    relative.removeSuffix(".jux").replace('/', '_') + ".jux"
                }
                check(seen.add(name)) { "two corpus files would share the name $name" }
                out += Entry(name, file)
            }
        return out
    }

    /** Just the files, for callers that do not configure them into a project. */
    fun files(root: File): List<File> = entries(root).map { it.file }

    /**
     * What the corpus tests sweep: everything under [examples], then the Jux
     * lesson programs (every `.jux` under `tests/lessons/<Lesson>/src`), 39 small
     * projects written the way a newcomer writes Jux, and so a different
     * test of the editor than the examples are.
     *
     * The lessons all have a `src/Main.jux`, so each lesson file is named
     * `lesson_<Lesson>_src_...jux`, clear of every example's name.
     */
    fun sweep(examples: File): List<Entry> {
        val lessons = File(examples.absoluteFile.parentFile, "tests/lessons")
        val lessonEntries = entries(lessons)
            .filter { it.file.relativeTo(lessons).invariantSeparatorsPath.split('/').getOrNull(1) == "src" }
            .map { Entry("lesson_" + it.name, it.file) }
        return entries(examples) + lessonEntries
    }

    /** Just the files of [sweep]. */
    fun sweepFiles(examples: File): List<File> = sweep(examples).map { it.file }

    /**
     * Every generated foreign declaration stub (`*.jux.d`) in the repository.
     *
     * Two kinds, and both matter:
     *
     *  - `crates/juxc-driver/stubs/rust-std.jux.d`, the standard library stub
     *    BUNDLED into every toolchain. It is parsed in every project, so one
     *    unparseable line in it breaks import resolution everywhere.
     *  - `examples/<project>/.jux-stubs/rust/<crate>.jux.d`, generated per project from the
     *    crates it binds. Present only on a machine that has built those
     *    examples, so the caller treats them as a bonus, not a requirement.
     *
     * Names are derived from the path (`rust_image.jux.d` and
     * `tiny_skia.jux.d` both exist under different projects), and the `.jux.d`
     * suffix is KEPT: it is what puts the parser into foreign-stub mode.
     */
    fun stubs(repoRoot: File): List<Entry> {
        if (!repoRoot.isDirectory) return emptyList()
        val out = mutableListOf<Entry>()
        val seen = mutableSetOf<String>()
        repoRoot.walkTopDown()
            .onEnter { dir -> dir == repoRoot || dir.name !in STUB_SKIP }
            .filter { it.isFile && it.name.endsWith(".jux.d") }
            .sortedBy { it.relativeTo(repoRoot).invariantSeparatorsPath }
            .forEach { file ->
                val relative = file.relativeTo(repoRoot).invariantSeparatorsPath
                val name = relative.removeSuffix(".jux.d").replace('/', '_').replace('.', '_') + ".jux.d"
                if (seen.add(name)) out += Entry(name, file)
            }
        return out
    }

    /**
     * Directories the stub walk never enters. `.jux-stubs` is deliberately NOT
     * here (unlike [SKIP]): generated stubs are exactly what this walk wants.
     * `target` and `.rust-build` hold cargo output, which on a developer
     * machine is gigabytes, and `.claude` can hold whole worktree copies of
     * the repo, whose stubs would just be the same files again.
     */
    private val STUB_SKIP = setOf("target", ".rust-build", "build", ".git", ".claude", "node_modules")
}
