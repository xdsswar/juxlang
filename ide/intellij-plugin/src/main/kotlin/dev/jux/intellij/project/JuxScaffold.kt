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
            JuxProjectKind.WORKSPACE -> return WORKSPACE_MANIFEST
        }
        return "$header$target\n\n[dependencies]\n"
    }

    /** `main.jux` for an executable, `lib.jux` for a library; a workspace has none. */
    fun entryFileName(kind: JuxProjectKind): String = when (kind) {
        JuxProjectKind.EXECUTABLE -> "main.jux"
        JuxProjectKind.LIBRARY -> "lib.jux"
        JuxProjectKind.WORKSPACE -> ""
    }

    /**
     * A workspace root, exactly as `jux new --workspace` writes it
     * (JUX-BUILD-SYSTEM-ADDENDUM §B.15.1, bin/jux `cmd_new`): an empty member
     * list with a pattern hint, and the two inheritance tables members read
     * with `key.workspace = true`.
     */
    const val WORKSPACE_MANIFEST =
        "[workspace]\n" +
            "# Member packages, relative to this file. Patterns work: \"tools/*\".\n" +
            "members = []\n" +
            "\n" +
            "[workspace.package]\n" +
            "edition = \"2026\"\n" +
            "\n" +
            "[workspace.dependencies]\n"

    /**
     * The package-path segment `jux new` derives from a directory name
     * (bin/jux `package_segment`): lower-case ASCII letters, digits and `_`,
     * not starting with a digit. `My-App` -> `my_app`.
     */
    fun packageSegment(name: String): String {
        val out = StringBuilder(name.map { if (it.isLetterOrDigit() && it.code < 128) it.lowercaseChar() else '_' }.joinToString(""))
        if (out.isEmpty() || out[0].isDigit()) out.insert(0, '_')
        return out.toString()
    }

    /** `my_lib` -> `MyLib`, the file name `jux new --lib` gives the first source. */
    fun typeNameFor(segment: String): String =
        segment.split('_').filter { it.isNotEmpty() }.joinToString("") { it.replaceFirstChar(Char::uppercaseChar) }
            .ifEmpty { "Lib" }

    /**
     * Every file of a new project of [kind] in directory [dirName], as
     * `jux new` lays it out (§B.15.1): the fallback the wizard writes when
     * no `jux` toolchain is installed to run `jux new` itself. A library puts
     * its code in its package directory under a package-less `src/lib.jux`,
     * with a first test; [crateType] other than `lib` is added to `[lib]`.
     */
    fun files(dirName: String, kind: JuxProjectKind, crateType: String, sample: Boolean): Map<String, String> {
        val out = LinkedHashMap<String, String>()
        out[".gitignore"] = "target/\n"
        when (kind) {
            JuxProjectKind.WORKSPACE -> {
                out["jux.toml"] = WORKSPACE_MANIFEST
                out["README.md"] = "# $dirName\n\nA Jux workspace.\n\nAdd a member with `jux new --lib <name>` inside this directory and list it in `members`.\n\n## Building\n\n    jux build\n"
            }
            JuxProjectKind.EXECUTABLE -> {
                out["jux.toml"] = newPackageManifest(dirName, lib = false, crateType = "lib")
                out["src/main.jux"] = entryContent(kind, sample)
                out["README.md"] = "# $dirName\n\nA Jux project.\n\n## Building\n\n    jux build\n\n## Running\n\n    jux run\n"
            }
            JuxProjectKind.LIBRARY -> {
                val segment = packageSegment(dirName)
                val type = typeNameFor(segment)
                out["jux.toml"] = newPackageManifest(segment, lib = true, crateType = crateType)
                out["src/lib.jux"] = "// Crate root of the `$segment` library. It stays package-less; the\n" +
                    "// library's code lives under src/$segment/ in `package $segment;`.\n"
                if (sample) {
                    out["src/$segment/$type.jux"] = "package $segment;\n\n" +
                        "/** A first public function; `import $segment.greet;` reaches it. */\n" +
                        "public String greet(String who) {\n    return \"Hello, \" + who + \"!\";\n}\n"
                    out["test/$segment/${type}Test.jux"] = "package $segment;\n\n" +
                        "import jux.std.testing.*;\n\n" +
                        "@Test\n" +
                        "void greetsByName() {\n    assertEqual(\"Hello, Jux!\", greet(\"Jux\"));\n}\n"
                } else {
                    out["src/$segment/$type.jux"] = "package $segment;\n"
                }
                out["README.md"] = "# $dirName\n\nA Jux project.\n\n## Building\n\n    jux build\n\n## Testing\n\n    jux test\n"
            }
        }
        return out
    }

    /** The `jux.toml` `jux new` writes for a package (bin/jux `package_manifest`). */
    private fun newPackageManifest(name: String, lib: Boolean, crateType: String): String {
        val libTable = when {
            !lib -> ""
            crateType == "lib" -> "\n[lib]\n"
            else -> "\n[lib]\ncrate-type = [\"$crateType\"]\n"
        }
        return "[package]\n" +
            "name = \"$name\"\n" +
            "version = \"0.1.0\"\n" +
            "edition = \"2026\"\n" +
            "authors = [\"Your Name <you@example.com>\"]\n" +
            "license = \"Apache-2.0\"\n" +
            libTable +
            "\n[build]\n" +
            "profile = \"full\"\n" +
            "target = \"native\"\n" +
            "optimization = \"release\"\n" +
            "\n[dependencies]\n"
    }

    /** Starter code for the entry file; minimal stub when [sample] is false. */
    fun entryContent(kind: JuxProjectKind, sample: Boolean): String = when (kind) {
        JuxProjectKind.WORKSPACE -> ""
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
