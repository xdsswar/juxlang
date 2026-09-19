package dev.jux.intellij.toml

import junit.framework.TestCase

/**
 * The line model of `jux.toml` and the workspace checks built on it
 * (JUX-BUILD-SYSTEM-ADDENDUM §B.7), over an in-memory directory tree.
 */
class JuxTomlModelTest : TestCase() {

    /** A directory tree given as the set of directories that hold a jux.toml, plus bare directories. */
    private class FakeDirs(private val packages: Set<String>, private val bare: Set<String> = emptySet()) : JuxTomlModel.DirView {
        private val all: Set<String> by lazy {
            val out = HashSet<String>()
            for (p in packages + bare) {
                var cur = p
                while (cur.isNotEmpty()) {
                    out.add(cur)
                    cur = cur.substringBeforeLast('/', "")
                }
            }
            out
        }
        override fun childDirs(rel: String): List<String> =
            all.filter { it.substringBeforeLast('/', "") == rel }.map { it.substringAfterLast('/') }
        override fun isDirectory(rel: String): Boolean = rel.isEmpty() || rel in all
        override fun hasManifest(rel: String): Boolean = rel in packages
    }

    private val root = """
        [workspace]
        members = ["app", "tools/*"]
        exclude = ["tools/old"]
        default-members = ["app"]

        [workspace.package]
        edition = "2026"
        license = "MIT"

        [workspace.dependencies]
        "com.x.json" = "1.0"

        [workspace.dependencies.rust.rand]
        version = "0.9"
    """.trimIndent()

    fun testEntriesKnowTheirTableAndKeys() {
        val text = "[package]\nname = \"app\"\nedition.workspace = true   # inherited\n\n[dependencies]\n\"com.x.json\".workspace = true\nlocal = { workspace = true, features = [\"a\"] }\n"
        val e = JuxTomlModel.entries(text)
        assertEquals(listOf("package", "package", "dependencies", "dependencies"), e.map { it.table })
        assertEquals(listOf("name", "edition.workspace", "com.x.json.workspace", "local"), e.map { it.key })
        assertEquals(listOf("name", "edition", "com.x.json", "local"), e.map { it.head })
        assertEquals(listOf(false, true, true, true), e.map { it.inheritsFromWorkspace })
        // The key span covers the head as written (quotes included for a quoted key).
        val dep = e[2]
        assertEquals("\"com.x.json\"", text.substring(dep.keyStart, dep.headEnd))
    }

    fun testMultiLineArraysAreOneEntry() {
        val text = "[workspace]\nmembers = [\n  \"a\",\n  \"b\", # comment\n]\nexclude = []\n"
        val e = JuxTomlModel.entries(text)
        assertEquals(listOf("members", "exclude"), e.map { it.key })
        assertEquals(listOf("a", "b"), JuxTomlModel.stringArray(e[0].value))
    }

    fun testWorkspaceRootFacts() {
        assertTrue(JuxTomlModel.isWorkspaceRoot(root))
        assertEquals(listOf("edition", "license"), JuxTomlModel.workspacePackageKeys(root))
        assertEquals(listOf("com.x.json", "rust.rand"), JuxTomlModel.workspaceDependencyNames(root))
        assertEquals(listOf("app", "tools/*"), JuxTomlModel.workspaceArray(root, "members"))
        val at = JuxTomlModel.workspaceDeclarationOffset(root, "package", "license")!!
        assertTrue(root.startsWith("license", at))
        val sub = JuxTomlModel.workspaceDeclarationOffset(root, "dependencies", "rust.rand")!!
        assertTrue(root.startsWith("[workspace.dependencies.rust.rand]", sub))
        assertNull(JuxTomlModel.workspaceDeclarationOffset(root, "package", "homepage"))
    }

    fun testTableAtOffset() {
        val text = "[package]\nname = \"x\"\n\n[workspace]\nmembers = []\n"
        assertEquals("package", JuxTomlModel.tableAt(text, text.indexOf("name")))
        assertEquals("workspace", JuxTomlModel.tableAt(text, text.indexOf("members")))
        assertEquals("", JuxTomlModel.tableAt("title = 1\n[package]\n", 0))
    }

    fun testMemberExpansionMirrorsTheDriver() {
        val dirs = FakeDirs(setOf("app", "tools/a", "tools/b", "tools/old"), bare = setOf("tools/scripts"))
        assertEquals(
            listOf("app", "tools/a", "tools/b"),
            JuxTomlModel.expandMembers(dirs, listOf("app", "tools/*"), listOf("tools/old")),
        )
        assertTrue(JuxTomlModel.pathMatches("tools/?", "tools/a"))
        assertFalse(JuxTomlModel.pathMatches("tools/*", "tools/a/deep"))
    }

    fun testChecksOnAWorkspaceRoot() {
        val text = """
            [workspace]
            members = ["app", "missing", "tools/*", "plain"]
            default-members = ["app", "ghost"]
            resolver = "2"
        """.trimIndent()
        val dirs = FakeDirs(setOf("app"), bare = setOf("plain"))
        val problems = JuxTomlChecks.check(text, dirs, text)
        val messages = problems.map { it.message }
        assertTrue(messages.toString(), messages.any { it.contains("Unknown key `resolver`") })
        assertTrue(messages.toString(), messages.any { it == "Workspace member `missing` does not exist" })
        assertTrue(messages.toString(), messages.any { it == "Workspace member `plain` has no jux.toml" })
        assertTrue(messages.toString(), messages.any { it.contains("`tools/*` matches no directory") })
        assertTrue(messages.toString(), messages.any { it == "`ghost` is not a workspace member" })
        assertFalse(messages.toString(), messages.any { it.contains("`app`") })
        // Each problem points at its own text.
        val ghost = problems.first { it.message.contains("ghost") }
        assertEquals("\"ghost\"", text.substring(ghost.start, ghost.end))
    }

    fun testInheritanceChecks() {
        val member = """
            [package]
            name = "app"
            edition.workspace = true
            homepage.workspace = true

            [dependencies]
            "com.x.json".workspace = true
            nope = { workspace = true }

            [profile.dev]
            opt-level.workspace = true
        """.trimIndent()
        val messages = JuxTomlChecks.check(member, FakeDirs(emptySet()), root).map { it.message }
        assertTrue(messages.toString(), messages.any { it == "`homepage` is not declared in the workspace root's [workspace.package]" })
        assertTrue(messages.toString(), messages.any { it == "`nope` is not declared in the workspace root's [workspace.dependencies]" })
        assertTrue(messages.toString(), messages.any { it.startsWith("Only [package] and [dependencies]") })
        assertFalse(messages.toString(), messages.any { it.contains("`edition`") || it.contains("com.x.json") })
    }

    fun testInheritingWithoutARoot() {
        val messages = JuxTomlChecks.check("[package]\nname = \"a\"\nedition.workspace = true\n", FakeDirs(emptySet()), null)
            .map { it.message }
        assertEquals(1, messages.size)
        assertTrue(messages[0], messages[0].contains("no jux.toml with [workspace]"))
    }

    fun testANameIsNeverInherited() {
        val messages = JuxTomlChecks.check("[package]\nname.workspace = true\n", FakeDirs(emptySet()), root).map { it.message }
        assertTrue(messages.toString(), messages.any { it.contains("`name` is its own") })
    }

    fun testCleanManifestHasNoProblems() {
        val member = "[package]\nname = \"app\"\nedition.workspace = true\n\n[dependencies]\n\"com.x.json\".workspace = true\n"
        assertEquals(emptyList<JuxTomlChecks.Problem>(), JuxTomlChecks.check(member, FakeDirs(emptySet()), root))
    }
}
