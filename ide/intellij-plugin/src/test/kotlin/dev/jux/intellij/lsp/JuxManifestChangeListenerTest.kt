package dev.jux.intellij.lsp

import junit.framework.TestCase

/**
 * The jux.toml-change trigger that drives dependency re-discovery. Only a
 * `jux.toml` inside the project should fire the LSP restart; source files and
 * out-of-project manifests must not. Path matching is separator-agnostic.
 */
class JuxManifestChangeListenerTest : TestCase() {

    private val base = "F:/DEV/proj"

    fun testRootManifestMatches() {
        assertTrue(JuxManifestChangeListener.isManifestPath("F:/DEV/proj/jux.toml", base))
    }

    fun testWorkspaceMemberManifestMatches() {
        assertTrue(JuxManifestChangeListener.isManifestPath("F:/DEV/proj/mod/jux.toml", base))
    }

    fun testBackslashPathsMatch() {
        assertTrue(JuxManifestChangeListener.isManifestPath("F:\\DEV\\proj\\app\\jux.toml", base))
    }

    fun testSourceFileDoesNotMatch() {
        assertFalse(JuxManifestChangeListener.isManifestPath("F:/DEV/proj/src/Main.jux", base))
    }

    fun testManifestOutsideProjectDoesNotMatch() {
        assertFalse(JuxManifestChangeListener.isManifestPath("F:/DEV/other/jux.toml", base))
    }

    /** A file merely ending in `jux.toml` (no path boundary) is not a manifest. */
    fun testNonBoundaryNameDoesNotMatch() {
        assertFalse(JuxManifestChangeListener.isManifestPath("F:/DEV/proj/src/notjux.toml", base))
    }

    /** Build output under target/ must not trigger a restart. */
    fun testManifestUnderTargetIsIgnored() {
        assertFalse(
            JuxManifestChangeListener.isManifestPath("F:/DEV/proj/target/.rust-build/x/jux.toml", base),
        )
    }

    /** Windows drive-letter case differences must still match. */
    fun testCaseInsensitiveDriveLetter() {
        assertTrue(JuxManifestChangeListener.isManifestPath("f:/DEV/proj/mod/jux.toml", "F:/DEV/proj"))
    }
}
