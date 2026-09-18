package dev.jux.intellij.project

import com.intellij.navigation.NavigationItem
import com.intellij.openapi.application.WriteAction
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.openapi.vfs.VirtualFileManager
import com.intellij.testFramework.IndexingTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Everything a project depends on is indexed: Jux path dependencies, git
 * dependencies checked out in the toolchain's cache, and the generated
 * `.jux.d` stubs of Rust crates. Their types and members complete (ranked
 * below the user's own code) and are found by Go to Class.
 *
 * And it stays indexed as things change: an edit inside a path dependency
 * shows at once, a new git checkout (`jux update`, another tag) replaces the
 * old root, and a change to a path-sourced crate regenerates its stub.
 *
 * Dependencies live outside the fixture's `/src` content root, in the same
 * in-memory file system, as they live outside the project on disk.
 */
class JuxDependencyIndexTest : BasePlatformTestCase() {

    private val created = ArrayList<VirtualFile>()

    override fun setUp() {
        super.setUp()
        JuxDependencySync.getInstance(project).resetForTests()
    }

    override fun tearDown() {
        try {
            JuxDependencies.gitCacheRootForTests = null
            JuxDependencySync.getInstance(project).resetForTests()
            WriteAction.runAndWait<Exception> { created.filter { it.isValid }.forEach { it.delete(this) } }
            created.clear()
            JuxDependencies.invalidate(project)
            JuxDependencySync.getInstance(project).announceRoots()
        } catch (e: Throwable) {
            addSuppressedException(e)
        } finally {
            super.tearDown()
        }
    }

    // ---- fixture helpers ----

    private fun tempRoot(): VirtualFile = VirtualFileManager.getInstance().findFileByUrl("temp:///")!!

    /** Create [relative] (under `temp:///`) with [text], outside the project content. */
    private fun outside(relative: String, text: String): VirtualFile = WriteAction.computeAndWait<VirtualFile, Exception> {
        val top = relative.substringBefore('/')
        val existing = tempRoot().findChild(top)
        val file = VfsUtil.createDirectoryIfMissing(tempRoot(), relative.substringBeforeLast('/'))!!
            .let { dir -> dir.findChild(relative.substringAfterLast('/')) ?: dir.createChildData(this, relative.substringAfterLast('/')) }
        VfsUtil.saveText(file, text)
        if (existing == null) tempRoot().findChild(top)?.let { created.add(it) }
        file
    }

    private fun write(file: VirtualFile, text: String) = WriteAction.runAndWait<Exception> { VfsUtil.saveText(file, text) }

    /** Re-read the manifests and announce the roots, then let indexing finish. */
    private fun sync() {
        JuxDependencySync.getInstance(project).flush()
        IndexingTestUtil.waitUntilIndexesAreReady(project)
    }

    private fun completion(code: String): List<String> {
        myFixture.configureByText("use.jux", code)
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    private fun gotoClass(name: String): List<String> =
        myFixture.getGotoClassResults(name, true, null).mapNotNull { (it as? NavigationItem)?.name }

    // ---- the three kinds of dependency ----

    private fun pathLib() {
        outside("deplibs/shapes/jux.toml", "[package]\nname = \"shapes\"\n")
        outside(
            "deplibs/shapes/src/geo.jux",
            "package shapes.geo;\n\npublic class Polygon {\n    public int sides() { return 3; }\n}\n",
        )
    }

    private fun gitLib(tag: String, className: String) {
        val cacheName = JuxDependencies.gitCacheDirName("https://example.com/acme/tools.git", "tag=$tag")
        outside("depcache/$cacheName/jux.toml", "[package]\nname = \"tools\"\n")
        outside(
            "depcache/$cacheName/src/kit.jux",
            "package tools.kit;\n\npublic class $className {\n    public int turns() { return 1; }\n}\n",
        )
        JuxDependencies.gitCacheRootForTests = tempRoot().findChild("depcache")
    }

    private fun manifest(tag: String = "v1") = myFixture.addFileToProject(
        "jux.toml",
        """
        [package]
        name = "app"

        [dependencies]
        shapes = { path = "../deplibs/shapes" }
        tools = { git = "https://example.com/acme/tools.git", tag = "$tag" }
        "rust.demo" = "1.0"
        """.trimIndent(),
    )

    private fun rustStub() {
        myFixture.addFileToProject(
            ".jux-stubs/rust/demo.jux.d",
            "// juxc crate stub cache-version 7 source registry\npackage rust.demo;\n\npublic class Plotter {\n    public int dots();\n}\n",
        )
    }

    fun testDependenciesAreReadFromTheManifests() {
        pathLib()
        gitLib("v1", "Wrench")
        manifest()
        sync()
        val snapshot = JuxDependencies.snapshot(project)
        val names = snapshot.dependencyPackages.map { it.name to it.origin }
        assertTrue("path dep: $names", "shapes" to JuxDependencies.Origin.PATH in names)
        assertTrue("git dep: $names", "tools" to JuxDependencies.Origin.GIT in names)
        assertEquals(listOf("demo"), snapshot.foreignDeps.map { it.name })
    }

    fun testDependencyTypesCompleteBelowTheUsersOwn() {
        pathLib()
        gitLib("v1", "Wrench")
        manifest()
        rustStub()
        myFixture.addFileToProject("mine.jux", "public class Pony { }\npublic class Wrangler { }\n")
        sync()
        val p = completion("void main() { P<caret> }")
        assertTrue("path-dep type: $p", "Polygon" in p)
        assertTrue("crate stub type: $p", "Plotter" in p)
        assertTrue("the user's own type ranks first: $p", p.indexOf("Pony") < p.indexOf("Polygon"))
        assertTrue("the user's own type ranks first: $p", p.indexOf("Pony") < p.indexOf("Plotter"))
        val w = completion("void main() { W<caret> }")
        assertTrue("git-dep type: $w", "Wrench" in w)
        assertTrue("the user's own type ranks first: $w", w.indexOf("Wrangler") < w.indexOf("Wrench"))
    }

    fun testDependencyMembersComplete() {
        pathLib()
        gitLib("v1", "Wrench")
        manifest()
        sync()
        val m = completion("import shapes.geo.Polygon;\nvoid main() { Polygon p = new Polygon(); p.<caret> }")
        assertTrue("path-dep member: $m", "sides" in m)
    }

    fun testDependencyTypesAreFoundByGoToClass() {
        pathLib()
        gitLib("v1", "Wrench")
        manifest()
        rustStub()
        sync()
        assertTrue(gotoClass("Polygon").contains("Polygon"))
        assertTrue(gotoClass("Wrench").contains("Wrench"))
        assertTrue(gotoClass("Plotter").contains("Plotter"))
    }

    // ---- re-indexing ----

    fun testEditInAPathDependencyShowsAtOnce() {
        pathLib()
        manifest()
        sync()
        val file = tempRoot().findFileByRelativePath("deplibs/shapes/src/geo.jux")!!
        write(
            file,
            "package shapes.geo;\n\npublic class Polygon {\n    public int sides() { return 3; }\n    public int area() { return 0; }\n}\n",
        )
        IndexingTestUtil.waitUntilIndexesAreReady(project)
        val m = completion("import shapes.geo.Polygon;\nvoid main() { Polygon p = new Polygon(); p.<caret> }")
        assertTrue("the new member is offered: $m", "area" in m)
    }

    fun testNewGitCheckoutReplacesTheOldRoot() {
        pathLib()
        gitLib("v1", "Wrench")
        manifest("v1")
        sync()
        assertTrue(gotoClass("Wrench").contains("Wrench"))
        // `tag = "v2"` and `jux update`: a new checkout directory.
        gitLib("v2", "Spanner")
        WriteAction.runAndWait<Exception> {
            VfsUtil.saveText(myFixture.findFileInTempDir("jux.toml"), manifestText("v2"))
        }
        sync()
        assertTrue("the new checkout is indexed", gotoClass("Spanner").contains("Spanner"))
        assertFalse("the old checkout is not a root any more", gotoClass("Wrench").contains("Wrench"))
    }

    private fun manifestText(tag: String) = """
        [package]
        name = "app"

        [dependencies]
        shapes = { path = "../deplibs/shapes" }
        tools = { git = "https://example.com/acme/tools.git", tag = "$tag" }
        "rust.demo" = "1.0"
    """.trimIndent()

    fun testCrateSourceEditRegeneratesItsStub() {
        val lib = outside("depcrates/fake/src/lib.rs", "pub fn v1() -> i32 { 1 }\n")
        outside("depcrates/fake/Cargo.toml", "[package]\nname = \"fake\"\n")
        myFixture.addFileToProject(
            "jux.toml",
            "[package]\nname = \"app\"\n\n[dependencies]\n\"rust.fake\" = { path = \"../depcrates/fake\" }\n",
        )
        val calls = ArrayList<List<String>>()
        var version = 1
        JuxDependencySync.getInstance(project).generatorForTests = JuxStubGenerator { _, deps, _ ->
            calls.add(deps.map { it.name })
            WriteAction.runAndWait<Exception> {
                for (dep in deps) {
                    val dir = VfsUtil.createDirectoryIfMissing(dep.owner.root, ".jux-stubs/${dep.kind}")!!
                    val stub = dir.findChild("${dep.name}.jux.d") ?: dir.createChildData(this, "${dep.name}.jux.d")
                    VfsUtil.saveText(
                        stub,
                        "// juxc crate stub cache-version 7 source ${dep.expectedSourceTag()}\n" +
                            "package rust.fake;\n\npublic class Fake {\n    public int v$version();\n}\n",
                    )
                }
            }
        }
        val sync = JuxDependencySync.getInstance(project)
        sync.rememberCurrentEntries()
        sync()
        assertEquals("a missing stub is generated", listOf(listOf("fake")), calls)
        assertTrue(completion("import rust.fake.*;\nvoid main() { Fake f = new Fake(); f.<caret> }").contains("v1"))

        // Nothing changed: no regeneration.
        sync()
        assertEquals(1, calls.size)

        // An edit to the crate's Rust sources regenerates its stub.
        version = 2
        write(lib, "pub fn v2() -> i32 { 2 }\n")
        sync()
        assertEquals("a source edit regenerates", 2, calls.size)
        IndexingTestUtil.waitUntilIndexesAreReady(project)
        val after = completion("import rust.fake.*;\nvoid main() { Fake f = new Fake(); f.<caret> }")
        assertTrue("the regenerated stub is what completes: $after", "v2" in after && "v1" !in after)
    }

    fun testManifestVersionChangeRegeneratesTheStub() {
        myFixture.addFileToProject(
            "jux.toml",
            "[package]\nname = \"app\"\n\n[dependencies]\n\"rust.demo\" = \"1.0\"\n",
        )
        rustStub()
        val calls = ArrayList<String>()
        JuxDependencySync.getInstance(project).generatorForTests = JuxStubGenerator { _, deps, _ ->
            deps.forEach { calls.add(it.name) }
        }
        val sync = JuxDependencySync.getInstance(project)
        sync.rememberCurrentEntries()
        sync()
        assertTrue("an up-to-date stub is left alone: $calls", calls.isEmpty())
        WriteAction.runAndWait<Exception> {
            VfsUtil.saveText(
                myFixture.findFileInTempDir("jux.toml"),
                "[package]\nname = \"app\"\n\n[dependencies]\n\"rust.demo\" = \"2.0\"\n",
            )
        }
        sync()
        assertEquals("a version bump regenerates", listOf("demo"), calls)
    }
}
