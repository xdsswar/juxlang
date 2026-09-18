package dev.jux.intellij.actions

import com.intellij.ide.fileTemplates.FileTemplateManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * New | Jux File (§I.5): every kind's template, with the `package` line the
 * target directory implies (§I.4), or none at a package root or outside every
 * root.
 */
class JuxNewFileTemplatesTest : BasePlatformTestCase() {

    /** Create [name] from [template] in the fixture directory [dirPath]; the file's text. */
    private fun create(template: String, name: String, dirPath: String): String {
        val vDir = myFixture.tempDirFixture.findOrCreateDir(dirPath)
        val dir = PsiManager.getInstance(project).findDirectory(vDir)!!
        var created: PsiFile? = null
        WriteCommandAction.runWriteCommandAction(project) { created = NewJuxFileAction.create(name, template, dir) }
        val file = created ?: error("template $template produced no file")
        assertEquals("$name.jux", file.name)
        return file.text
    }

    /** The text after the license header the templates include. */
    private fun body(text: String): String {
        assertTrue("missing header in:\n$text", text.startsWith("/*"))
        return text.substringAfter("*/").trimStart('\r', '\n').replace("\r\n", "\n")
    }

    fun testEveryTemplateTheDialogOffersExists() {
        val manager = FileTemplateManager.getInstance(project)
        for (name in NewJuxFileAction.TEMPLATE_NAMES) {
            assertNotNull("internal template $name", manager.getInternalTemplate(name))
        }
    }

    fun testEveryKindInAPackage() {
        val kinds = linkedMapOf(
            "Jux Class" to "public class Cart {\n}",
            "Jux Interface" to "public interface Priced {\n}",
            "Jux Enum" to "public enum Size {\n}",
            "Jux Struct" to "public struct Point {\n}",
            "Jux Record" to "public record Line() {\n}",
            "Jux Annotation" to "public annotation Audited {\n}",
        )
        for ((template, declaration) in kinds) {
            val name = declaration.substringAfter(' ').substringAfter(' ').substringBefore(' ').substringBefore('(')
            val text = body(create(template, name, "com/acme"))
            assertEquals(template, "package com.acme;\n\n$declaration", text.trimEnd())
        }
    }

    fun testPlainFileCarriesOnlyThePackageLine() {
        assertEquals("package com.acme;", body(create("Jux File", "notes", "com/acme")).trimEnd())
    }

    fun testFileAtARootHasNoPackageLine() {
        val text = body(create("Jux Class", "Top", ""))
        assertFalse(text, "package" in text)
        assertEquals("public class Top {\n}", text.trimEnd())
    }

    fun testManifestProjectDecidesThePackage() {
        myFixture.addFileToProject("shop/jux.toml", "")
        assertEquals("package com.acme;\n\npublic class Cart {\n}", body(create("Jux Class", "Cart", "shop/src/com/acme")).trimEnd())
        // Directly in src/: no package (§B.1.1).
        assertEquals("public class Main {\n}", body(create("Jux Class", "Main", "shop/src")).trimEnd())
        // examples/ stand alone, so their files get no inferred package (§B.1.3).
        assertEquals("public class Demo {\n}", body(create("Jux Class", "Demo", "shop/examples")).trimEnd())
    }
}
