package dev.jux.intellij.run

import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import com.intellij.testIntegration.TestCreator
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * "Create New Test..." from Navigate | Test (Ctrl+Shift+T) on a source file
 * that has none: writes `test/a/b/FooTest.jux` beside `src/a/b/Foo.jux`
 * (JUX-BUILD-SYSTEM-ADDENDUM §B.1.2) and opens it.
 *
 * The file follows §TS: package `a.b.test`, the `jux.std.testing`
 * assertions imported, the tested type imported, and one `@Test` free
 * function per public method of the type (per public free function of a file
 * without one), each empty and ready to fill in. An existing test file is
 * opened, never overwritten.
 */
class JuxTestCreator : TestCreator {

    override fun isAvailable(project: Project, editor: Editor?, file: PsiFile): Boolean {
        if (file !is JuxFile) return false
        val vFile = file.virtualFile ?: return false
        return JuxTestLayout.root(vFile, "src") != null && !JuxTestFinder().isTest(file)
    }

    override fun createTest(project: Project, editor: Editor?, file: PsiFile) {
        val created = create(project, file as? JuxFile ?: return) ?: return
        val caret = created.second
        FileEditorManager.getInstance(project).openTextEditor(OpenFileDescriptor(project, created.first, caret), true)
    }

    /**
     * Create (or find) the test file for [source]; returns it and the offset
     * to put the caret at. Null when [source] is not under a `src/` root.
     */
    fun create(project: Project, source: JuxFile): Pair<VirtualFile, Int>? {
        val vFile = source.virtualFile ?: return null
        val src = JuxTestLayout.root(vFile, "src") ?: return null
        val projectDir = src.parent ?: return null
        val rel = JuxTestLayout.relativeDir(vFile, src)
        val name = "${vFile.nameWithoutExtension}Test.jux"
        return WriteCommandAction.writeCommandAction(project).withName("Create Test").compute<Pair<VirtualFile, Int>?, RuntimeException> {
            val dir = VfsUtil.createDirectoryIfMissing(projectDir, listOf("test", rel).filter { it.isNotEmpty() }.joinToString("/"))
                ?: return@compute null
            dir.findChild(name)?.let { return@compute it to 0 }
            val text = testText(source)
            val out = dir.createChildData(this, name)
            VfsUtil.saveText(out, text)
            // The caret goes inside the first test's body.
            out to text.indexOf("{\n").let { if (it < 0) 0 else it + 2 }
        }
    }

    /** The new test file's text. */
    fun testText(source: JuxFile): String {
        val pkg = JuxTestDetector.packageName(source)
        val base = source.virtualFile?.nameWithoutExtension ?: source.name.removeSuffix(".jux")
        val type = source.children.filterIsInstance<JuxTypeDeclaration>().firstOrNull { it.name == base }
            ?: source.children.filterIsInstance<JuxTypeDeclaration>().firstOrNull()
        val tested = testedNames(source, type).ifEmpty { listOf(base) }
        val sb = StringBuilder()
        if (pkg.isNotEmpty()) sb.append("package ").append(pkg).append(".test;\n\n")
        sb.append("import jux.std.testing.*;\n")
        if (type?.name != null && pkg.isNotEmpty()) sb.append("import ").append(pkg).append('.').append(type.name).append(";\n")
        for (name in tested) {
            sb.append("\n@Test\nvoid test").append(name.replaceFirstChar { it.uppercaseChar() }).append("() {\n}\n")
        }
        return sb.toString()
    }

    /** The public methods of [type], or the public free functions of [file] when it has none. */
    private fun testedNames(file: JuxFile, type: JuxTypeDeclaration?): List<String> {
        val candidates = if (type != null) JuxHierarchy.allMembersDeclaredIn(type) else file.children.toList()
        return candidates
            .filter { it.elementType === E.METHOD_DECLARATION && (type == null || JuxHierarchy.hasModifier(it, "public")) }
            .mapNotNull { (it as? JuxNamedElement)?.name }
            .filter { it != "main" }
            .distinct()
    }
}
