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

    fun testWorkspaceMembers() {
        val ws = """
            [workspace]
            members = ["core", "apps/*"]
        """.trimIndent()
        assertEquals(listOf("core", "apps/*"), JuxToml.workspaceMembers(ws))
    }
}
