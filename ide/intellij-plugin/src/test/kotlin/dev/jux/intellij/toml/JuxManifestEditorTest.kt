package dev.jux.intellij.toml

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * `jux.toml` in the editor: completion of the `[workspace]` keys, member
 * paths and inherited entries; the workspace inspection; and Ctrl+B from a
 * `key.workspace = true` entry to the root's declaration.
 */
class JuxManifestEditorTest : BasePlatformTestCase() {

    private val root = """
        [workspace]
        members = ["app", "tools/*"]

        [workspace.package]
        edition = "2026"
        license = "MIT"

        [workspace.dependencies]
        "com.x.json" = "1.0"
    """.trimIndent()

    private fun addWorkspace() {
        myFixture.addFileToProject("jux.toml", root)
        myFixture.addFileToProject("app/src/main.jux", "void main() {}")
        myFixture.addFileToProject("tools/a/jux.toml", "[package]\nname = \"a\"\n")
        myFixture.addFileToProject("tools/b/jux.toml", "[package]\nname = \"b\"\n")
    }

    private fun openMember(text: String) {
        val file = myFixture.addFileToProject("app/jux.toml", text)
        myFixture.configureFromExistingVirtualFile(file.virtualFile)
    }

    fun testWorkspaceKeysComplete() {
        myFixture.configureByText("jux.toml", "[workspace]\ndef<caret>\n")
        myFixture.completeBasic()
        val text = myFixture.editor.document.text
        assertTrue(text, text.contains("default-members = []"))
    }

    fun testInheritedPackageKeysComplete() {
        addWorkspace()
        openMember("[package]\nname = \"app\"\nlic<caret>\n")
        myFixture.completeBasic()
        val text = myFixture.editor.document.text
        assertTrue(text, text.contains("license.workspace = true"))
    }

    fun testInheritedDependenciesComplete() {
        addWorkspace()
        openMember("[package]\nname = \"app\"\n\n[dependencies]\n<caret>\n")
        val items = myFixture.completeBasic()?.map { it.lookupString }.orEmpty()
        assertTrue(items.toString(), items.contains("\"com.x.json\".workspace = true"))
    }

    fun testMemberPathsCompleteInsideTheArray() {
        addWorkspace()
        // The root, with the caret at the start of the "app" string.
        myFixture.configureFromTempProjectFile("jux.toml")
        myFixture.editor.caretModel.moveToOffset(myFixture.editor.document.text.indexOf("\"app\"") + 1)
        val items = myFixture.completeBasic()?.map { it.lookupString }.orEmpty()
        assertTrue(items.toString(), items.contains("tools/a"))
        assertTrue(items.toString(), items.contains("tools/*"))
    }

    fun testInspectionReportsAnUndeclaredInheritedKey() {
        addWorkspace()
        openMember("[package]\nname = \"app\"\nhomepage.workspace = true\nedition.workspace = true\n")
        myFixture.enableInspections(JuxManifestInspection())
        val messages = myFixture.doHighlighting().mapNotNull { it.description }
        assertTrue(messages.toString(), messages.any { it.contains("`homepage` is not declared") })
        assertFalse(messages.toString(), messages.any { it.contains("`edition`") })
    }

    fun testGotoInheritedKeyLandsOnTheRootDeclaration() {
        addWorkspace()
        openMember("[package]\nname = \"app\"\nlicen<caret>se.workspace = true\n")
        val targets = JuxManifestGotoHandler().getGotoDeclarationTargets(
            myFixture.file.findElementAt(myFixture.caretOffset) ?: myFixture.file,
            myFixture.caretOffset,
            myFixture.editor,
        )
        assertNotNull(targets)
        val target = targets!!.single() as JuxManifestGotoHandler.ManifestKeyTarget
        // The root manifest, not the member's own.
        assertEquals("jux.toml", target.containingFile.name)
        assertFalse(target.containingFile.virtualFile.parent.name == "app")
        assertTrue(root.startsWith("license", target.offset()))
    }
}
