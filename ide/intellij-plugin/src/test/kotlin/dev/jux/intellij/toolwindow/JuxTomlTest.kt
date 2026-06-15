package dev.jux.intellij.toolwindow

import junit.framework.TestCase

/** [JuxToml] — the minimal manifest reader behind the Jux Project tree. */
class JuxTomlTest : TestCase() {

    private val exe = """
        [package]
        name = "com.example.app"
        version = "0.1.0"
        edition = "2026"

        [[bin]]
        name = "app"

        [dependencies]
        greeter = { path = "../greeter" }
        "rust.serde_json" = "1.0"
        net = { git = "https://x/y", branch = "main" }
    """.trimIndent()

    private val lib = """
        [package]
        name = "com.example.lib"
        version = "2.3.4"

        [lib]
        crate-type = ["lib", "cdylib"]

        [dependencies]
    """.trimIndent()

    fun testPackageFields() {
        assertEquals("com.example.app", JuxToml.packageName(exe))
        assertEquals("0.1.0", JuxToml.packageVersion(exe))
        assertEquals("2026", JuxToml.edition(exe))
    }

    fun testLibDetection() {
        assertFalse(JuxToml.hasLib(exe))
        assertTrue(JuxToml.hasLib(lib))
        assertEquals(listOf("lib", "cdylib"), JuxToml.libCrateTypes(lib))
        assertTrue(JuxToml.libCrateTypes(exe).isEmpty())
    }

    fun testBins() {
        assertEquals(listOf("app"), JuxToml.bins(exe))
        assertTrue(JuxToml.bins(lib).isEmpty())
    }

    fun testDependencyDetails() {
        val d = JuxToml.dependencyDetails(exe).toMap()
        assertEquals("path: ../greeter", d["greeter"])
        assertEquals("1.0", d["rust.serde_json"])
        assertEquals("git: https://x/y (main)", d["net"])
        assertTrue(JuxToml.dependencyDetails(lib).isEmpty())
    }

    /** `[dependencies.NAME]` sub-tables must appear alongside inline entries. */
    fun testDependencySubTables() {
        val text = """
            [package]
            name = "com.example.app"

            [dependencies]
            greeter = { path = "../greeter" }
            "rust.serde_json" = "1.0"

            [dependencies.serde]
            version = "1.0"
            features = ["derive"]

            [dependencies.local]
            path = "../local"
        """.trimIndent()
        val names = JuxToml.dependencies(text)
        assertTrue("inline + sub-table names", names.containsAll(
            listOf("greeter", "rust.serde_json", "serde", "local")))
        val d = JuxToml.dependencyDetails(text).toMap()
        assertEquals("path: ../greeter", d["greeter"])
        assertEquals("1.0", d["rust.serde_json"])
        assertEquals("1.0", d["serde"])
        assertEquals("path: ../local", d["local"])
    }

    fun testWorkspaceMembers() {
        val ws = """
            [workspace]
            members = ["core", "apps/*"]
        """.trimIndent()
        assertEquals(listOf("core", "apps/*"), JuxToml.workspaceMembers(ws))
    }

    /** `[lib] name` is surfaced for the targets group; null when unset. */
    fun testLibName() {
        val named = """
            [lib]
            name = "mylib"
        """.trimIndent()
        assertEquals("mylib", JuxToml.libName(named))
        assertNull(JuxToml.libName(lib))
        assertNull(JuxToml.libName(exe))
    }

    /** `[build] target` drives the tool window's default cross-compile triple. */
    fun testBuildTarget() {
        val t = """
            [build]
            target = "x86_64-unknown-linux-gnu"
        """.trimIndent()
        assertEquals("x86_64-unknown-linux-gnu", JuxToml.buildTarget(t))
        assertNull(JuxToml.buildTarget(exe))
    }

    // ---- adding a workspace member (New Module action) -----------------------

    /** No `[workspace]` yet → the section is appended with the new member. */
    fun testAddMemberCreatesWorkspaceSection() {
        val out = JuxToml.withWorkspaceMember(exe, "core")!!
        assertTrue("section appended", out.contains("[workspace]"))
        assertEquals(listOf("core"), JuxToml.workspaceMembers(out))
        // The original package stays intact.
        assertEquals("com.example.app", JuxToml.packageName(out))
    }

    /** Existing members array → the new member is appended to it. */
    fun testAddMemberExtendsExistingArray() {
        val ws = """
            [workspace]
            members = ["core"]
        """.trimIndent()
        val out = JuxToml.withWorkspaceMember(ws, "app")!!
        assertEquals(listOf("core", "app"), JuxToml.workspaceMembers(out))
    }

    /** Already a member → no change (null). */
    fun testAddMemberAlreadyPresentIsNoOp() {
        val ws = """
            [workspace]
            members = ["core", "app"]
        """.trimIndent()
        assertNull(JuxToml.withWorkspaceMember(ws, "app"))
    }

    /** `[workspace]` present but empty → a `members` array is created. */
    fun testAddMemberCreatesArrayInEmptyWorkspace() {
        val ws = """
            [package]
            name = "com.example.root"

            [workspace]
        """.trimIndent()
        val out = JuxToml.withWorkspaceMember(ws, "core")!!
        assertEquals(listOf("core"), JuxToml.workspaceMembers(out))
    }
}
