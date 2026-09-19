package dev.jux.intellij.run

import java.io.File

/**
 * What a run configuration needs to know about a project's `jux.toml`, read
 * straight from the manifest text and the project directory so the settings
 * editor fills its pickers without starting a process. Pure: no IntelliJ
 * types, so it unit-tests without a fixture.
 *
 * Mirrors the build system's own reading (JUX-BUILD-SYSTEM-ADDENDUM §B.9 and
 * §B.1.3): the four built-in profiles always exist, a `[profile.<name>]` table
 * adds (or overrides) one, and an example is a `.jux` file or a directory
 * holding `.jux` files under `examples/`.
 */
object JuxManifestInfo {

    /** Profiles every project has, whether or not `jux.toml` mentions them (§B.9). */
    val BUILTIN_PROFILES: List<String> = listOf("dev", "release", "test", "bench")

    // `[profile.fast]`, `[profile."size-opt"]`, `[ profile . fast ]`.
    private val PROFILE_HEADER = Regex("""^\s*\[\s*profile\s*\.\s*"?([A-Za-z0-9_\-]+)"?\s*]\s*$""")

    /**
     * The profile names a `--profile` picker should offer: the built-ins
     * first, in the driver's order, then every custom `[profile.<name>]` in
     * the order the manifest declares them. Never duplicates a name.
     */
    fun profiles(manifestText: String): List<String> {
        val out = LinkedHashSet(BUILTIN_PROFILES)
        for (line in manifestText.lineSequence()) {
            PROFILE_HEADER.matchEntire(line.substringBefore('#'))?.let { out.add(it.groupValues[1]) }
        }
        return out.toList()
    }

    /**
     * The example names under `<root>/examples/`, sorted, exactly as `jux run
     * --example <name>` names them: a `foo.jux` file is `foo`, a directory
     * with at least one `.jux` file somewhere inside is its directory name.
     * Hidden entries and `target/` are skipped, like the driver does.
     */
    fun examples(root: File): List<String> {
        val dir = File(root, "examples")
        val entries = dir.listFiles() ?: return emptyList()
        val out = ArrayList<String>()
        for (entry in entries) {
            val name = entry.name
            if (name.startsWith('.') || name == "target") continue
            if (entry.isDirectory) {
                if (entry.walkTopDown().any { it.isFile && it.extension == "jux" }) out.add(name)
            } else if (entry.extension == "jux") {
                out.add(entry.nameWithoutExtension)
            }
        }
        return out.sorted()
    }
}
