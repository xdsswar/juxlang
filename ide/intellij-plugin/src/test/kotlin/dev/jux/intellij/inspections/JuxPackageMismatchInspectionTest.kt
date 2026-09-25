package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The §I.4 package-mismatch inspection and its three fixes, in a `jux.toml`
 * project (the manifest rule of `JuxPackageResolver`), plus the places it must
 * stay quiet.
 */
class JuxPackageMismatchInspectionTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(JuxPackageMismatchInspection())
        myFixture.addFileToProject("proj/jux.toml", "[package]\nname = \"proj\"\n")
    }

    /** Add [text] at [path] and open it, with the caret where `<caret>` is (or at the start). */
    private fun open(path: String, text: String) {
        val caret = text.indexOf("<caret>").coerceAtLeast(0)
        val file = myFixture.addFileToProject(path, text.replace("<caret>", ""))
        myFixture.configureFromExistingVirtualFile(file.virtualFile)
        myFixture.editor.caretModel.moveToOffset(caret)
    }

    /** The descriptions this inspection produced on the open file. */
    private fun problems(): List<String> = myFixture.doHighlighting()
        .mapNotNull { it.description }
        .filter { "package" in it.lowercase() && ("location" in it || "source root" in it) }

    fun testWrongPackageIsReportedAndRenamed() {
        open("proj/src/com/acme/Cart.jux", "package <caret>com.wrong;\n\npublic class Cart {}\n")
        assertEquals(
            listOf("Package name `com.wrong` does not correspond to the file location. Expected `com.acme`"),
            problems(),
        )
        myFixture.launchAction(myFixture.findSingleIntention("Set package name to `com.acme`"))
        myFixture.checkResult("package com.acme;\n\npublic class Cart {}\n")
        assertEmpty(problems())
    }

    fun testWrongPackageFileMovesToItsDeclaredDirectory() {
        open("proj/src/com/acme/Order.jux", "package <caret>com.wrong;\n\npublic class Order {}\n")
        myFixture.launchAction(myFixture.findSingleIntention("Move file to `com/wrong/`"))
        assertNotNull(myFixture.findFileInTempDir("proj/src/com/wrong/Order.jux"))
        assertNull(myFixture.findFileInTempDir("proj/src/com/acme/Order.jux"))
    }

    fun testMissingPackageIsAdded() {
        open("proj/src/com/acme/Line.jux", "public class Line {}\n")
        assertEquals(listOf("Missing package declaration: this file's location puts it in `com.acme`"), problems())
        myFixture.launchAction(myFixture.findSingleIntention("Add `package com.acme;`"))
        myFixture.checkResult("package com.acme;\n\npublic class Line {}\n")
    }

    fun testMissingPackageGoesBelowAHeaderComment() {
        open("proj/src/com/acme/Note.jux", "// header\n<caret>public class Note {}\n")
        myFixture.launchAction(myFixture.findSingleIntention("Add `package com.acme;`"))
        myFixture.checkResult("// header\npackage com.acme;\n\npublic class Note {}\n")
    }

    fun testRootFileMustHaveNoPackage() {
        open("proj/src/main.jux", "package <caret>app;\n\nvoid main() {}\n")
        assertEquals(
            listOf("A file directly in the source root has no package; `app` does not correspond to the file location"),
            problems(),
        )
        myFixture.launchAction(myFixture.findSingleIntention("Remove package declaration"))
        myFixture.checkResult("void main() {}\n")
    }

    fun testMoveIsNotOfferedOverAnExistingFile() {
        myFixture.addFileToProject("proj/src/com/wrong/Clash.jux", "package com.wrong;\n\npublic class Clash {}\n")
        open("proj/src/com/acme/Clash.jux", "package <caret>com.wrong;\n\npublic class Clash {}\n")
        assertEmpty(myFixture.filterAvailableIntentions("Move file to"))
        assertNotEmpty(myFixture.filterAvailableIntentions("Set package name to"))
    }

    fun testMatchingPackagesAreQuiet() {
        open("proj/src/com/acme/Ok.jux", "package com.acme;\n\npublic class Ok {}\n")
        assertEmpty(problems())
        open("proj/src/main.jux", "void main() {}\n")
        assertEmpty(problems())
    }

    fun testTestRootAcceptsBothConventions() {
        open("proj/test/com/acme/CartTest.jux", "package com.acme.test;\n\n@Test\nvoid adds() {}\n")
        assertEmpty(problems())
        open("proj/test/com/acme/LineTest.jux", "package com.acme;\n\n@Test\nvoid lines() {}\n")
        assertEmpty(problems())
    }

    /**
     * §B.15.2's canonical multi-binary project: a `[lib]` plus several
     * `[[bin]]`, with the extra entries under `src/bin/`.
     */
    private fun multiBinaryProject() {
        myFixture.addFileToProject(
            "multi/jux.toml",
            """
            [package]
            name = "multi"

            [lib]
            name = "multi"

            [[bin]]
            name = "server"
            path = "src/bin/server.jux"

            [[bin]]
            name = "migrator"
            path = "src/bin/migrator.jux"
            """.trimIndent() + "\n",
        )
        myFixture.addFileToProject("multi/src/lib.jux", "public class Shared {}\n")
    }

    fun testABinEntryFileNeedsNoPackage() {
        // ERRATA E103: a `[[bin]] path` entry file is a program's entry point,
        // not a member of the package tree, so `src/bin/server.jux` owes no
        // `package bin;`. The compiler used to demand it (`E0301`) on the shape
        // §B.15.2 documents as ordinary; the editor must not ask either.
        multiBinaryProject()
        open("multi/src/bin/server.jux", "void main() { print(1); }\n")
        assertEmpty(problems())
    }

    fun testAnOrdinarySourceBesideAnEntryStillDeclaresItsPackage() {
        // The exemption is per FILE, not per directory (E103), so a helper class
        // sitting beside an entry in `src/bin/` is an ordinary source.
        multiBinaryProject()
        open("multi/src/bin/Helper.jux", "public class Helper {}\n")
        assertEquals(
            listOf("Missing package declaration: this file's location puts it in `bin`"),
            problems(),
        )
    }

    fun testAnEntryFilesDeclaredPackageIsStillChecked() {
        // Only the REQUIREMENT is lifted (E103): a package the entry file does
        // declare is still checked against its directory.
        multiBinaryProject()
        open("multi/src/bin/server.jux", "package <caret>wrong;\n\nvoid main() {}\n")
        assertEquals(
            listOf("Package name `wrong` does not correspond to the file location. Expected `bin`"),
            problems(),
        )
    }

    fun testFilesWithoutAnAuthoritativeRootAreQuiet() {
        // The light fixture's plain source root: a guess, never enforced.
        open("loose/com/acme/Loose.jux", "package somewhere.else;\n\npublic class Loose {}\n")
        assertEmpty(problems())
        // A manifest project's examples/ stand alone (§B.1.3). (With no src/,
        // the manifest directory itself would be the root.)
        myFixture.tempDirFixture.findOrCreateDir("proj/src")
        open("proj/examples/basic.jux", "package demo;\n\nvoid main() {}\n")
        assertEmpty(problems())
    }
}
