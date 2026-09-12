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
}
