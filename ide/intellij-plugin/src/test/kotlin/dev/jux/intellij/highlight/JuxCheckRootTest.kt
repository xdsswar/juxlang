package dev.jux.intellij.highlight

import junit.framework.TestCase
import java.io.File
import java.nio.file.Files

/**
 * The root [JuxSemanticAnnotator] hands `juxc --check`.
 *
 * Mirrors `project_scope` in `crates/juxc-lsp/src/analysis.rs`: the nearest
 * ancestor with a `jux.toml`, widened to an enclosing `[workspace]` root when
 * one declares members. Without the widening a workspace member was checked
 * alone, could not see its sibling packages or their `.jux-stubs/`, and every
 * import of a sibling came back "unresolved import" while quick-doc on the
 * same word resolved fine.
 *
 * A plain JUnit case: `checkRoot` is filesystem-only, so it needs no IDE
 * fixture and stays fast.
 */
class JuxCheckRootTest : TestCase() {

    private lateinit var tmp: File

    override fun setUp() {
        super.setUp()
        tmp = Files.createTempDirectory("jux-checkroot").toFile()
    }

    override fun tearDown() {
        try {
            tmp.deleteRecursively()
        } finally {
            super.tearDown()
        }
    }

    /** Create `<tmp>/<relative>` with [text], parents included. */
    private fun write(relative: String, text: String): File {
        val f = File(tmp, relative)
        f.parentFile.mkdirs()
        f.writeText(text)
        return f
    }

    private fun checkRoot(file: File): String? = JuxSemanticAnnotator().checkRoot(file.path)

    fun testWidensToTheWorkspaceRoot() {
        write("ws/jux.toml", "[workspace]\nmembers = [\"ui\", \"app\"]\n")
        write("ws/ui/jux.toml", "[package]\nname = \"ui\"\n")
        val source = write("ws/ui/src/main.jux", "package ui;\n")
        assertEquals(File(tmp, "ws").path, checkRoot(source))
    }

    /** A standalone package is its own root: nothing above it declares members. */
    fun testStandalonePackageIsItsOwnRoot() {
        write("solo/jux.toml", "[package]\nname = \"solo\"\n")
        val source = write("solo/src/main.jux", "package solo;\n")
        assertEquals(File(tmp, "solo").path, checkRoot(source))
    }

    /** No manifest anywhere: the file's own directory, which still gets same-package files right. */
    fun testNoManifestFallsBackToTheFileDirectory() {
        val source = write("loose/main.jux", "void main() {}\n")
        assertEquals(File(tmp, "loose").path, checkRoot(source))
    }

    /**
     * A `[workspace]` section with no `members` key does not widen: an ancestor
     * manifest may carry workspace-wide defaults (`[workspace.dependencies]`,
     * `[workspace.package]`) without owning the package below it.
     */
    fun testWorkspaceSectionWithoutMembersDoesNotWiden() {
        write("outer/jux.toml", "[package]\nname = \"outer\"\n\n[workspace]\n")
        write("outer/inner/jux.toml", "[package]\nname = \"inner\"\n")
        val source = write("outer/inner/src/main.jux", "package inner;\n")
        assertEquals(File(tmp, "outer/inner").path, checkRoot(source))
    }

    /** A `members` key belonging to some other table is not the workspace's. */
    fun testMembersUnderAnotherTableDoesNotWiden() {
        write("outer/jux.toml", "[package]\nname = \"outer\"\nmembers = [\"nope\"]\n")
        write("outer/inner/jux.toml", "[package]\nname = \"inner\"\n")
        val source = write("outer/inner/src/main.jux", "package inner;\n")
        assertEquals(File(tmp, "outer/inner").path, checkRoot(source))
    }

    /** A commented-out `members` is not a declaration. */
    fun testCommentedMembersDoesNotWiden() {
        write("outer/jux.toml", "[workspace]\n# members = [\"inner\"]\n")
        write("outer/inner/jux.toml", "[package]\nname = \"inner\"\n")
        val source = write("outer/inner/src/main.jux", "package inner;\n")
        assertEquals(File(tmp, "outer/inner").path, checkRoot(source))
    }

    /** The outermost declaring workspace wins, as the language server's walk does. */
    fun testWidensPastAnIntermediateDirectory() {
        write("ws/jux.toml", "[workspace]\nmembers = [\"packages/ui\"]\n")
        write("ws/packages/ui/jux.toml", "[package]\nname = \"ui\"\n")
        val source = write("ws/packages/ui/src/main.jux", "package ui;\n")
        assertEquals(File(tmp, "ws").path, checkRoot(source))
    }
}
