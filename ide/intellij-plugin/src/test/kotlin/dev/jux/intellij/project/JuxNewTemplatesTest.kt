package dev.jux.intellij.project

import junit.framework.TestCase

/**
 * The New Project wizard's fallback templates ([JuxScaffold.files]) write
 * exactly what `jux new` writes (bin/jux `cmd_new`, JUX-BUILD-SYSTEM-ADDENDUM
 * §B.15.1) for a binary, a library and a workspace.
 */
class JuxNewTemplatesTest : TestCase() {

    fun testBinaryLayout() {
        val files = JuxScaffold.files("hello", JuxProjectKind.EXECUTABLE, "lib", sample = true)
        assertEquals(setOf(".gitignore", "jux.toml", "src/main.jux", "README.md"), files.keys)
        assertTrue(files.getValue("jux.toml").contains("name = \"hello\""))
        assertFalse(files.getValue("jux.toml").contains("[lib]"))
        assertTrue(files.getValue("jux.toml").contains("[build]\nprofile = \"full\""))
        assertTrue(files.getValue("src/main.jux").contains("public void main()"))
    }

    fun testLibraryLayoutPutsCodeInItsPackage() {
        val files = JuxScaffold.files("My-Geo", JuxProjectKind.LIBRARY, "lib", sample = true)
        assertEquals(
            setOf(".gitignore", "jux.toml", "src/lib.jux", "src/my_geo/MyGeo.jux", "test/my_geo/MyGeoTest.jux", "README.md"),
            files.keys,
        )
        val toml = files.getValue("jux.toml")
        assertTrue(toml, toml.contains("name = \"my_geo\""))
        assertTrue(toml, toml.contains("\n[lib]\n"))
        assertFalse(toml, toml.contains("crate-type"))
        assertTrue(files.getValue("src/my_geo/MyGeo.jux").startsWith("package my_geo;"))
        assertTrue(files.getValue("test/my_geo/MyGeoTest.jux").contains("@Test"))
    }

    fun testLibraryCrateTypeIsCarried() {
        val toml = JuxScaffold.files("ffi", JuxProjectKind.LIBRARY, "cdylib", sample = true).getValue("jux.toml")
        assertTrue(toml, toml.contains("[lib]\ncrate-type = [\"cdylib\"]"))
    }

    fun testWorkspaceRoot() {
        val files = JuxScaffold.files("mono", JuxProjectKind.WORKSPACE, "lib", sample = true)
        assertEquals(setOf(".gitignore", "jux.toml", "README.md"), files.keys)
        val toml = files.getValue("jux.toml")
        assertTrue(toml, toml.contains("[workspace]\n"))
        assertTrue(toml, toml.contains("members = []"))
        assertTrue(toml, toml.contains("[workspace.package]\nedition = \"2026\""))
        assertTrue(toml, toml.contains("[workspace.dependencies]"))
        assertEquals(JuxScaffold.WORKSPACE_MANIFEST, JuxScaffold.manifest("x", JuxProjectKind.WORKSPACE, "lib"))
    }

    fun testSegmentsAndTypeNamesMatchJuxNew() {
        assertEquals("my_app", JuxScaffold.packageSegment("my-app"))
        assertEquals("_2d", JuxScaffold.packageSegment("2d"))
        assertEquals("MyApp", JuxScaffold.typeNameFor("my_app"))
        assertEquals("Lib", JuxScaffold.typeNameFor("_"))
    }
}
