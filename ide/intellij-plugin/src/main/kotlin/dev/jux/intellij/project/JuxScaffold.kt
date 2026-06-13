package dev.jux.intellij.project

/**
 * Pure project-scaffold content — the manifest, entry file name, and starter
 * code for a new Jux project. Kept free of platform types so it's unit-testable
 * without a wizard/module fixture; [JuxModuleBuilder] just writes what these
 * return.
 */
internal object JuxScaffold {

    const val GITIGNORE = "/target/\n*.exe\n"

    /**
     * A valid reverse-DNS `package.name` (§B.2) from a module name: lowercased,
     * non-alphanumerics → `_`, leading digits/underscores trimmed, prefixed
     * under `com.example.` so it always has ≥2 segments.
     */
    fun packageNameFor(moduleName: String): String {
        val cleaned = moduleName.lowercase().map { if (it.isLetterOrDigit()) it else '_' }.joinToString("")
        val safe = cleaned.trimStart('_', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9').ifEmpty { "app" }
        return "com.example.$safe"
    }

    /** The `jux.toml` for a project of [kind] (library declares `[lib]`). */
    fun manifest(pkg: String, kind: JuxProjectKind, crateType: String): String {
        val header =
            """
            [package]
            name = "$pkg"
            version = "0.1.0"
            edition = "2026"
            """.trimIndent()
        // Executable: the default [[bin]] is inferred from src/main.jux, so no
        // explicit target is needed. Library: declare [lib] + crate-type.
        val target = when (kind) {
            JuxProjectKind.EXECUTABLE -> ""
            JuxProjectKind.LIBRARY -> "\n\n[lib]\ncrate-type = [\"$crateType\"]"
        }
        return "$header$target\n\n[dependencies]\n"
    }

    /** `main.jux` for an executable, `lib.jux` for a library. */
    fun entryFileName(kind: JuxProjectKind): String = when (kind) {
        JuxProjectKind.EXECUTABLE -> "main.jux"
        JuxProjectKind.LIBRARY -> "lib.jux"
    }

    /** Starter code for the entry file; minimal stub when [sample] is false. */
    fun entryContent(kind: JuxProjectKind, sample: Boolean): String = when (kind) {
        JuxProjectKind.EXECUTABLE ->
            if (sample) {
                "public void main() {\n    print(\"Hello, Jux!\");\n}\n"
            } else {
                "public void main() {\n}\n"
            }
        JuxProjectKind.LIBRARY ->
            if (sample) {
                "// Library entry point. Public items declared here form your crate's API.\n" +
                    "public class Greeter {\n" +
                    "    public String greet(String who) {\n" +
                    "        return \$\"Hello, \${who}!\";\n" +
                    "    }\n" +
                    "}\n"
            } else {
                "// Library entry point. Declare your public API here.\n"
            }
    }
}
