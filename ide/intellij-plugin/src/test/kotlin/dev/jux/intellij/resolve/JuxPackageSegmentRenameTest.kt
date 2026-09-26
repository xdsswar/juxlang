package dev.jux.intellij.resolve

import com.intellij.psi.PsiDirectory
import com.intellij.refactoring.rename.RenameUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Renaming a package directory follows ERRATA E78, not the identifier rule
 * [JuxNamesValidator.isIdentifier] applies to a declaration: a keyword is a
 * legal segment after the first one (`package demo.type;`), is not one at the
 * head of the path, and `self`/`Self`/`crate`/`super` are never one.
 */
class JuxPackageSegmentRenameTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.addFileToProject("proj/jux.toml", "[package]\nname = \"proj\"\n")
        myFixture.addFileToProject("proj/src/demo/model/Holder.jux", "package demo.model;\npublic class Holder {}\n")
    }

    private fun dir(path: String): PsiDirectory {
        val vf = myFixture.findFileInTempDir(path) ?: error("no directory $path")
        return psiManager.findDirectory(vf) ?: error("no PsiDirectory for $path")
    }

    private fun valid(path: String, newName: String) = RenameUtil.isValidName(project, dir(path), newName)

    fun testAKeywordIsALegalInnerSegment() {
        assertTrue(valid("proj/src/demo/model", "type"))
        assertTrue(valid("proj/src/demo/model", "record"))
        assertTrue(valid("proj/src/demo/model", "entities"))
    }

    fun testTheFirstSegmentFollowsTheIdentifierRule() {
        assertFalse(valid("proj/src/demo", "type"))
        assertTrue(valid("proj/src/demo", "app"))
    }

    fun testRustPathWordsAreNeverASegment() {
        for (word in listOf("self", "Self", "crate", "super")) {
            assertFalse(word, valid("proj/src/demo/model", word))
        }
    }

    fun testAMalformedNameIsRefused() {
        assertFalse(valid("proj/src/demo/model", "9lives"))
        assertFalse(valid("proj/src/demo/model", "a-b"))
    }

    fun testADeclarationStillRefusesKeywords() {
        val names = JuxNamesValidator()
        assertFalse(names.isIdentifier("type", project))
        assertFalse(names.isIdentifier("record", project))
        assertTrue(names.isIdentifier("total", project))
    }
}
