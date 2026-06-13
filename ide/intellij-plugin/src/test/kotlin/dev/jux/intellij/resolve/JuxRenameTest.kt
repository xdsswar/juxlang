package dev.jux.intellij.resolve

import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.intellij.util.containers.MultiMap
import dev.jux.intellij.psi.JuxFieldDeclaration
import dev.jux.intellij.psi.JuxLocalVariable
import dev.jux.intellij.psi.JuxNamedElement

/**
 * In-file Rename: usages follow the declaration automatically (they resolve
 * through the reference contributor), and [JuxRenamePsiElementProcessor] reports
 * a same-scope name collision before the edit is applied.
 */
class JuxRenameTest : BasePlatformTestCase() {

    fun testRenameLocalUpdatesAllUsages() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {
                public int go() {
                    var count = 1;
                    return count + count;
                }
            }
            """.trimIndent(),
        )
        myFixture.renameElement(namedLocal("count"), "total")
        val text = myFixture.file.text
        assertTrue("declaration renamed: $text", text.contains("var total = 1;"))
        assertTrue("every usage renamed: $text", text.contains("return total + total;"))
        assertFalse("no stale name remains: $text", text.contains("count"))
    }

    fun testRenameFieldUpdatesUsages() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {
                private int width;
                public int area(int h) { return width * h; }
            }
            """.trimIndent(),
        )
        val field = PsiTreeUtil.findChildOfType(myFixture.file, JuxFieldDeclaration::class.java)!!
        myFixture.renameElement(field, "w")
        val text = myFixture.file.text
        assertTrue("field decl renamed: $text", text.contains("private int w;"))
        assertTrue("field usage renamed: $text", text.contains("return w * h;"))
    }

    fun testSameScopeCollisionReported() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {
                public void go() {
                    var count = 1;
                    var other = 2;
                    print(count + other);
                }
            }
            """.trimIndent(),
        )
        val count = namedLocal("count")
        val conflicts = MultiMap<PsiElement, String>()
        JuxRenamePsiElementProcessor().findExistingNameConflicts(count, "other", conflicts)
        assertFalse("renaming 'count' onto sibling 'other' must conflict", conflicts.isEmpty)
    }

    fun testNonCollidingRenameHasNoConflict() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {
                public void go() {
                    var count = 1;
                    print(count);
                }
            }
            """.trimIndent(),
        )
        val count = namedLocal("count")
        val conflicts = MultiMap<PsiElement, String>()
        JuxRenamePsiElementProcessor().findExistingNameConflicts(count, "total", conflicts)
        assertTrue("a free name is not a conflict", conflicts.isEmpty)
    }

    fun testProcessorClaimsNamedElementsOnly() {
        myFixture.configureByText("a.jux", "package demo;\npublic class A { private int x; }\n")
        val processor = JuxRenamePsiElementProcessor()
        val field = PsiTreeUtil.findChildOfType(myFixture.file, JuxFieldDeclaration::class.java)!!
        assertTrue(processor.canProcessElement(field))
        assertFalse(processor.canProcessElement(myFixture.file))
    }

    private fun namedLocal(name: String): JuxNamedElement =
        PsiTreeUtil.findChildrenOfType(myFixture.file, JuxLocalVariable::class.java)
            .first { it.name == name }
}
