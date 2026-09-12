package dev.jux.intellij.resolve

import com.intellij.openapi.command.WriteCommandAction
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The per-file caches behind the project-wide symbol net, pinned.
 *
 * `JuxUnresolvedReferenceInspection` asks for every declared name in the
 * project on every daemon pass, and the daemon runs on every keystroke. That
 * question used to be answered by walking the full PSI tree of every `.jux`
 * file in the project, each time — so the cost of typing one character grew
 * with the size of the user's project, and showed up as highlighting that
 * lags behind the cursor.
 *
 * The walk is now cached per file, keyed on the file itself. These tests
 * assert the two halves of that contract, without timing anything: the cache
 * is REUSED when nothing changed, and DROPPED for the file that did change.
 * A timing assertion would be flaky; identity is exact.
 */
class JuxTypeIndexCachingTest : BasePlatformTestCase() {

    fun testNamesAreCachedUntilTheFileChanges() {
        val psi = myFixture.configureByText(
            "Cached.jux",
            """
            public class Alpha {
                private int count;
                public int count() { return this.count; }
            }
            """.trimIndent(),
        )

        val first = JuxTypeIndex.namesIn(psi)
        val second = JuxTypeIndex.namesIn(psi)
        assertSame("an unchanged file must not be walked twice", first, second)
        assertTrue("the class name is indexed", "Alpha" in first)
        assertTrue("its members are indexed", "count" in first)

        // Edit the file: the cache for THIS file must go.
        WriteCommandAction.runWriteCommandAction(project) {
            myFixture.editor.document.insertString(0, "public class Beta { }\n")
            PsiDocumentManager.getInstance(project).commitAllDocuments()
        }

        val third = JuxTypeIndex.namesIn(psi)
        assertNotSame("an edited file must be re-walked", first, third)
        assertTrue("the new declaration is indexed", "Beta" in third)
    }

    fun testEditingOneFileKeepsAnotherFilesCache() {
        // The point of keying each file's cache on that file: a keystroke in
        // one file must not invalidate the other N-1 files in the project.
        // Keying the whole net on `PsiModificationTracker.MODIFICATION_COUNT`
        // alone is what made every keystroke an O(project) walk.
        val untouched = myFixture.addFileToProject(
            "Untouched.jux",
            "public class Gamma { public void go() { } }",
        )
        val edited = myFixture.configureByText("Edited.jux", "public class Delta { }")

        val before = JuxTypeIndex.namesIn(untouched)
        assertTrue("Gamma" in before)

        WriteCommandAction.runWriteCommandAction(project) {
            myFixture.editor.document.insertString(0, "public class Epsilon { }\n")
            PsiDocumentManager.getInstance(project).commitAllDocuments()
        }
        // Touch the edited file's cache so the modification really landed.
        assertTrue("Epsilon" in JuxTypeIndex.namesIn(edited))

        val after = JuxTypeIndex.namesIn(untouched)
        assertSame("an untouched file keeps its cached names", before, after)
    }

    fun testProjectWideNamesSeeEveryFile() {
        myFixture.addFileToProject("One.jux", "public class One { }")
        myFixture.addFileToProject("Two.jux", "public interface Two { void go(); }")
        myFixture.configureByText("Three.jux", "public enum Three { A, B }")

        val names = JuxTypeIndex.declaredNames(
            project,
            GlobalSearchScope.allScope(project),
        )
        assertTrue("class from another file", "One" in names)
        assertTrue("interface from another file", "Two" in names)
        assertTrue("enum in the open file", "Three" in names)
        assertTrue("a method is a symbol too", "go" in names)
    }
}
