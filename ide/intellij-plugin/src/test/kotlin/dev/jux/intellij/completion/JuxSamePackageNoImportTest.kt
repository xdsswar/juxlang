package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Auto-import never writes an `import` for a type that is already visible.
 *
 * A type declared in the current file, or in the current file's package (in
 * any file of that package), resolves by itself: Jux binds the file's own
 * package into scope. An `import` for one is redundant, and the owner does not
 * want it appearing. `juxc-lsp` guarantees the same on its side
 * (`same_package_type_is_offered_no_import` in `crates/juxc-lsp/src/tests.rs`,
 * which pins both a sibling in the same package and a class importing itself);
 * this is the plugin half of that rule.
 *
 * The rule is checked at the ACCEPT, not at the offer: a same-package type is
 * still completed, it just inserts bare.
 */
class JuxSamePackageNoImportTest : BasePlatformTestCase() {

    /**
     * Complete `Widget` at the caret and return the resulting file text.
     * The lookup either auto-inserts (one match) or is accepted explicitly.
     */
    private fun completeWidget(code: String): String {
        myFixture.configureByText("App.jux", code)
        val items = myFixture.completeBasic()
        if (items != null && items.isNotEmpty()) {
            val widget = items.firstOrNull { it.lookupString == "Widget" }
            assertNotNull("Widget should be offered: ${myFixture.lookupElementStrings}", widget)
            myFixture.lookup.currentItem = widget
            myFixture.finishLookup('\n')
        }
        return myFixture.file.text
    }

    private val use = """
        public class App {
            public void go() {
                var w = new Widg<caret>
            }
        }
    """.trimIndent()

    fun testSamePackageTypeInsertsNoImport() {
        myFixture.addFileToProject("Widget.jux", "package app;\npublic class Widget {}\n")
        val text = completeWidget("package app;\n\n$use")
        assertFalse("a sibling in the same package needs no import:\n$text", text.contains("import"))
    }

    fun testSameFileTypeInsertsNoImport() {
        val text = completeWidget("package app;\n\npublic class Widget {}\n\n$use")
        assertFalse("a type in this very file needs no import:\n$text", text.contains("import"))
    }

    fun testDifferentPackageTypeStillInsertsTheImport() {
        myFixture.addFileToProject("lib/Widget.jux", "package lib;\npublic class Widget {}\n")
        val text = completeWidget("package app;\n\n$use")
        assertTrue("a cross-package type still imports:\n$text", text.contains("import lib.Widget;"))
    }

    /**
     * A file with no `package` line is not automatically in the default
     * package: its package comes from where it sits under its source root, so
     * a sibling of a package-declaring file in the same directory still needs
     * no import.
     */
    fun testFileWithoutAPackageLineTakesItsPackageFromItsLocation() {
        myFixture.addFileToProject("app/Widget.jux", "package app;\npublic class Widget {}\n")
        // The using file must really sit in `app/`, so `configureByText` (which
        // only takes a bare name) is not enough: add it, then open it and put
        // the caret where the marker was.
        val caret = use.indexOf("<caret>")
        val file = myFixture.addFileToProject("app/App.jux", use.replace("<caret>", ""))
        myFixture.configureFromExistingVirtualFile(file.virtualFile)
        myFixture.editor.caretModel.moveToOffset(caret)

        // The rule this case exists for, stated directly: no `package` line,
        // yet the file belongs to `app` because that is where it sits.
        assertEquals("app", JuxAutoImport.effectivePackageOfFile(myFixture.file))

        val items = myFixture.completeBasic()
        if (items != null && items.isNotEmpty()) {
            val widget = items.firstOrNull { it.lookupString == "Widget" }
            assertNotNull("Widget should be offered: ${myFixture.lookupElementStrings}", widget)
            myFixture.lookup.currentItem = widget
            myFixture.finishLookup('\n')
        }
        assertFalse(
            "a file with no package line is still in its directory's package:\n${myFixture.file.text}",
            myFixture.file.text.contains("import"),
        )
    }

    /** Two files in the default package do not import each other either. */
    fun testDefaultPackageTypesDoNotImportEachOther() {
        myFixture.addFileToProject("Widget.jux", "public class Widget {}\n")
        val text = completeWidget(use)
        assertFalse("the default package needs no imports:\n$text", text.contains("import"))
    }
}
