package dev.jux.intellij.project

import com.intellij.openapi.vfs.VirtualFile
import com.intellij.testFramework.PsiTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.JuxPackageResolver
import dev.jux.intellij.JuxPackageResolver.Kind

/**
 * The §I.4 project model: which directory is a file's package root, the
 * package that implies, the root kinds, the `jux.toml` detector, and the
 * `jux.sourceRoots` list the language server is given.
 *
 * The light fixture's `src/` is an ordinary (non-Jux) source root, so a file
 * dropped straight into it exercises the fallback rule, and a `jux.toml`
 * placed below it exercises the manifest rule.
 */
class JuxPackageModelTest : BasePlatformTestCase() {

    private fun file(path: String, text: String = ""): VirtualFile = myFixture.addFileToProject(path, text).virtualFile

    private fun dir(path: String): VirtualFile = myFixture.tempDirFixture.findOrCreateDir(path)

    private fun infer(file: VirtualFile): String? = JuxPackageResolver.inferPackage(file, project)

    private fun kind(file: VirtualFile): Kind? = JuxPackageResolver.rootFor(file, project)?.kind

    private fun expected(file: VirtualFile): List<String>? = JuxPackageResolver.expectedPackages(file, project)

    fun testLooseFileUsesTheFallbackRootAndIsNotChecked() {
        val loose = file("com/acme/Loose.jux")
        assertEquals("com.acme", infer(loose))
        assertEquals(Kind.FALLBACK, kind(loose))
        // A guess is good for a template, never for the mismatch inspection.
        assertNull(expected(loose))
    }

    fun testManifestSrcIsTheSourcesRoot() {
        file("shop/jux.toml")
        val cart = file("shop/src/com/acme/Cart.jux")
        val main = file("shop/src/main.jux")
        assertEquals("com.acme", infer(cart))
        assertEquals(Kind.MANIFEST_SOURCES, kind(cart))
        assertEquals(listOf("com.acme"), expected(cart))
        // Directly in src/: no package (§B.1.1).
        assertEquals("", infer(main))
        assertEquals(listOf(""), expected(main))
    }

    fun testManifestTestRootAcceptsTheTestSuffix() {
        file("shop/jux.toml")
        dir("shop/src")
        val test = file("shop/test/com/acme/CartTest.jux")
        assertEquals(Kind.MANIFEST_TESTS, kind(test))
        assertEquals(listOf("com.acme", "com.acme.test"), expected(test))
    }

    fun testFilesOutsideSrcAndTestHaveNoRoot() {
        file("shop/jux.toml")
        file("shop/src/main.jux")
        // Each example stands alone and takes its package from its declaration (§B.1.3).
        val example = file("shop/examples/basic.jux")
        assertNull(JuxPackageResolver.rootFor(example, project))
        assertNull(infer(example))
        assertNull(expected(example))
    }

    fun testManifestWithoutSrcIsItsOwnRoot() {
        file("lib/jux.toml")
        val strings = file("lib/util/Strings.jux")
        assertEquals("util", infer(strings))
        assertEquals(Kind.MANIFEST_SOURCES, kind(strings))
    }

    fun testNearestManifestWins() {
        // A workspace manifest above member manifests: each member owns its files.
        file("ws/jux.toml")
        file("ws/app/jux.toml")
        val app = file("ws/app/src/demo/App.jux")
        assertEquals("demo", infer(app))
        assertEquals(dir("ws/app/src"), JuxPackageResolver.rootFor(app, project)?.dir)
    }

    fun testMarkedRootsWinOverTheManifest() {
        file("marked/jux.toml")
        val root = dir("marked/code")
        val tests = dir("marked/checks")
        PsiTestUtil.addSourceRoot(module, root, JuxSourceRootType.SOURCE)
        PsiTestUtil.addSourceRoot(module, tests, JuxSourceRootType.TEST_SOURCE)
        try {
            val f = file("marked/code/p/q/F.jux")
            assertEquals("p.q", infer(f))
            assertEquals(Kind.MARKED_SOURCES, kind(f))
            val t = file("marked/checks/p/FTest.jux")
            assertEquals(Kind.MARKED_TESTS, kind(t))
            assertEquals(listOf("p", "p.test"), expected(t))
        } finally {
            PsiTestUtil.removeSourceRoot(module, root)
            PsiTestUtil.removeSourceRoot(module, tests)
        }
    }

    fun testDirectoryForCreatesThePackagePath() {
        file("pkg/jux.toml")
        val main = file("pkg/src/main.jux")
        val root = JuxPackageResolver.rootFor(main, project)!!
        assertNull(JuxPackageResolver.directoryFor(root, "a.b", create = false))
        val made = com.intellij.openapi.application.WriteAction.computeAndWait<VirtualFile?, Throwable> {
            JuxPackageResolver.directoryFor(root, "a.b", create = true)
        }
        assertEquals(dir("pkg/src/a/b"), made)
        assertEquals(root.dir, JuxPackageResolver.directoryFor(root, "", create = false))
    }

    fun testDetectorFindsAndMarksSrcAndTest() {
        file("app/jux.toml")
        file("app/src/main.jux")
        file("app/test/AppTest.jux")
        file("app/docs/notes.jux")
        val found = JuxSourceRootDetector.unmarkedRoots(project)
        assertEquals(
            listOf("app/src" to JuxSourceRootType.SOURCE, "app/test" to JuxSourceRootType.TEST_SOURCE),
            found.map { "${it.dir.parent.name}/${it.dir.name}" to it.type },
        )
        JuxSourceRootDetector.mark(found)
        try {
            assertEquals(Kind.MARKED_SOURCES, kind(file("app/src/x/X.jux")))
            assertEquals(Kind.MARKED_TESTS, kind(file("app/test/x/XTest.jux")))
            assertEmpty(JuxSourceRootDetector.unmarkedRoots(project))
            // Marked roots reach the language server's list, once each.
            val roots = JuxSourceRootsConfig.compute(project)
            val src = roots.single { it["path"] == dir("app/src").path }
            assertEquals("sources", src["kind"])
            assertEquals("marked", src["origin"])
            assertEquals("tests", roots.single { it["path"] == dir("app/test").path }["kind"])
        } finally {
            PsiTestUtil.removeSourceRoot(module, dir("app/src"))
            PsiTestUtil.removeSourceRoot(module, dir("app/test"))
        }
    }

    fun testSourceRootsConfigListsManifestRootsWithNothingMarked() {
        file("svc/jux.toml")
        file("svc/src/main.jux")
        file("svc/test/SvcTest.jux")
        file("bare/jux.toml")
        val roots = JuxSourceRootsConfig.compute(project)
        assertEquals("manifest", roots.single { it["path"] == dir("svc/src").path }["origin"])
        assertEquals("tests", roots.single { it["path"] == dir("svc/test").path }["kind"])
        // A manifest with no src/ is its own root.
        assertEquals("sources", roots.single { it["path"] == dir("bare").path }["kind"])
        // The section the server asks for, and the whole `jux` section.
        assertEquals(roots, JuxSourceRootsConfig.answer(project, "jux.sourceRoots"))
        assertEquals(mapOf("sourceRoots" to roots), JuxSourceRootsConfig.answer(project, "jux"))
        assertNull(JuxSourceRootsConfig.answer(project, "rust-analyzer"))
    }

    fun testSourceRootKindsAreRegisteredForPersistence() {
        // The serializer is what writes a marked root to the .iml and reads it back.
        val ids = JuxJpsModelSerializerExtension().moduleSourceRootPropertiesSerializers.associate { it.typeId to it.type }
        assertEquals(JuxSourceRootType.SOURCE, ids[JuxSourceRootType.SOURCE_ID])
        assertEquals(JuxSourceRootType.TEST_SOURCE, ids[JuxSourceRootType.TEST_SOURCE_ID])
        assertTrue(JuxSourceRootType.TEST_SOURCE.isForTests)
        assertFalse(JuxSourceRootType.SOURCE.isForTests)
        assertEquals("Jux Sources Root", JuxSourceRootEditHandler().fullRootTypeName)
        assertEquals("Jux Test Sources Root", JuxTestSourceRootEditHandler().fullRootTypeName)
    }
}
