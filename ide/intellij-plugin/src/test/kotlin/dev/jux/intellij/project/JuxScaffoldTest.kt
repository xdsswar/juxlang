package dev.jux.intellij.project

import junit.framework.TestCase

/**
 * [JuxScaffold] — the manifest, entry file, and starter code chosen by the
 * New Project wizard for executable vs library projects.
 */
class JuxScaffoldTest : TestCase() {

    fun testExecutableManifestHasNoLibTarget() {
        val m = JuxScaffold.manifest("com.example.app", JuxProjectKind.EXECUTABLE, "lib")
        assertTrue(m.contains("[package]"))
        assertTrue(m.contains("name = \"com.example.app\""))
        assertFalse("executable manifest must not declare [lib]", m.contains("[lib]"))
        assertTrue(m.contains("[dependencies]"))
    }

    fun testLibraryManifestDeclaresLibAndCrateType() {
        val m = JuxScaffold.manifest("com.example.lib", JuxProjectKind.LIBRARY, "cdylib")
        assertTrue(m.contains("[lib]"))
        assertTrue("crate-type carried through", m.contains("crate-type = [\"cdylib\"]"))
    }

    fun testEntryFileNames() {
        assertEquals("main.jux", JuxScaffold.entryFileName(JuxProjectKind.EXECUTABLE))
        assertEquals("lib.jux", JuxScaffold.entryFileName(JuxProjectKind.LIBRARY))
    }

    fun testExecutableSampleHasMain() {
        val c = JuxScaffold.entryContent(JuxProjectKind.EXECUTABLE, sample = true)
        assertTrue(c.contains("public void main()"))
        assertTrue(c.contains("print("))
    }

    fun testLibrarySampleExposesPublicApi() {
        val c = JuxScaffold.entryContent(JuxProjectKind.LIBRARY, sample = true)
        assertTrue(c.contains("public class"))
    }

    fun testEmptyStubsOmitSampleBody() {
        assertFalse(JuxScaffold.entryContent(JuxProjectKind.EXECUTABLE, sample = false).contains("print("))
        assertFalse(JuxScaffold.entryContent(JuxProjectKind.LIBRARY, sample = false).contains("public class"))
    }

    fun testPackageNameIsReverseDnsSafe() {
        assertEquals("com.example.my_app", JuxScaffold.packageNameFor("My-App"))
        assertEquals("com.example.app", JuxScaffold.packageNameFor("123"))
    }
}
