package dev.jux.intellij.run

import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import com.intellij.psi.search.FilenameIndex
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testIntegration.TestFinder
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * Navigate | Test (Ctrl+Shift+T) between a source file and its tests, by the
 * layout of JUX-BUILD-SYSTEM-ADDENDUM §B.1.2: `test/a/b/FooTest.jux` tests
 * `src/a/b/Foo.jux`. A test file is found by name anywhere in the project
 * (`FooTest.jux`, `FooTests.jux`); the one mirroring the source's own path
 * comes first, as the one the build system associates with it.
 */
class JuxTestFinder : TestFinder {

    /** Where "go to test" starts from: the top-level type at the caret, else its file. */
    override fun findSourceElement(from: PsiElement): PsiElement? {
        val file = from.containingFile as? JuxFile ?: return null
        return PsiTreeUtil.getTopmostParentOfType(from, JuxTypeDeclaration::class.java) ?: file
    }

    override fun findTestsForClass(element: PsiElement): Collection<PsiElement> {
        val file = element.containingFile as? JuxFile ?: return emptyList()
        val vFile = file.virtualFile ?: return emptyList()
        val bases = linkedSetOf(vFile.nameWithoutExtension)
        (element as? JuxTypeDeclaration)?.name?.let { bases.add(it) }
        val names = bases.flatMap { base -> TEST_SUFFIXES.map { "$base$it.jux" } }
        val mirror = JuxTestLayout.testPathFor(vFile)
        return findFiles(file, names)
            .filter { it != vFile }
            .sortedBy { if (it.path == mirror) 0 else 1 }
            .mapNotNull { primaryElement(file, it) }
    }

    override fun findClassesForTest(element: PsiElement): Collection<PsiElement> {
        val file = element.containingFile as? JuxFile ?: return emptyList()
        val vFile = file.virtualFile ?: return emptyList()
        val base = JuxTestLayout.testedName(vFile.nameWithoutExtension) ?: return emptyList()
        val mirror = JuxTestLayout.sourcePathFor(vFile)
        return findFiles(file, listOf("$base.jux"))
            .filter { it != vFile && !JuxTestLayout.isUnderTestRoot(it) }
            .sortedBy { if (it.path == mirror) 0 else 1 }
            .mapNotNull { primaryElement(file, it) }
    }

    /**
     * A test is a file under a `test/` root, a file named like one
     * (`FooTest.jux`), or a file with `@Test` functions in it.
     */
    override fun isTest(element: PsiElement): Boolean {
        val file = element.containingFile as? JuxFile ?: return false
        val vFile = file.virtualFile
        if (vFile != null && (JuxTestLayout.isUnderTestRoot(vFile) || JuxTestLayout.testedName(vFile.nameWithoutExtension) != null)) {
            return true
        }
        return JuxTestDetector.hasTests(file)
    }

    private fun findFiles(context: PsiFile, names: List<String>): List<VirtualFile> {
        val scope = GlobalSearchScope.projectScope(context.project)
        return names.flatMap { FilenameIndex.getVirtualFilesByName(it, scope) }.distinct()
    }

    /** A file's type of the same name when it declares one, else the file. */
    private fun primaryElement(context: PsiFile, vFile: VirtualFile): PsiElement? {
        val psi = PsiManager.getInstance(context.project).findFile(vFile) as? JuxFile ?: return null
        return psi.children.filterIsInstance<JuxTypeDeclaration>().firstOrNull { it.name == vFile.nameWithoutExtension } ?: psi
    }

    private companion object {
        val TEST_SUFFIXES = listOf("Test", "Tests")
    }
}

/**
 * The §B.1.2 layout: `src/` holds sources, a sibling `test/` mirrors it, and
 * `FooTest.jux` tests `Foo.jux`.
 */
object JuxTestLayout {
    private val SUFFIXES = listOf("Tests", "Test")

    /** `Foo` for `FooTest` / `FooTests`, or null when the name is not a test's. */
    fun testedName(fileBase: String): String? =
        SUFFIXES.firstOrNull { fileBase.endsWith(it) && fileBase.length > it.length }?.let { fileBase.removeSuffix(it) }

    /** The nearest ancestor directory named [name], or null. */
    fun root(file: VirtualFile, name: String): VirtualFile? {
        var dir = file.parent
        while (dir != null) {
            if (dir.name == name) return dir
            dir = dir.parent
        }
        return null
    }

    fun isUnderTestRoot(file: VirtualFile): Boolean = root(file, "test") != null

    /** The directories from a root down to [file]'s own, `a/b` for `src/a/b/Foo.jux`. */
    fun relativeDir(file: VirtualFile, root: VirtualFile): String {
        val dir = file.parent ?: return ""
        return if (dir == root) "" else dir.path.removePrefix(root.path).trimStart('/')
    }

    /** Where `src/a/b/Foo.jux`'s test lives: `test/a/b/FooTest.jux`, as a path. */
    fun testPathFor(source: VirtualFile): String? {
        val src = root(source, "src") ?: return null
        val rel = relativeDir(source, src)
        val base = src.parent?.path ?: return null
        return listOf(base, "test", rel, "${source.nameWithoutExtension}Test.jux").filter { it.isNotEmpty() }.joinToString("/")
    }

    /** Where `test/a/b/FooTest.jux`'s source lives: `src/a/b/Foo.jux`, as a path. */
    fun sourcePathFor(test: VirtualFile): String? {
        val testRoot = root(test, "test") ?: return null
        val name = testedName(test.nameWithoutExtension) ?: return null
        val rel = relativeDir(test, testRoot)
        val base = testRoot.parent?.path ?: return null
        return listOf(base, "src", rel, "$name.jux").filter { it.isNotEmpty() }.joinToString("/")
    }
}
