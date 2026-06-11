package dev.jux.intellij.resolve

import com.intellij.psi.search.GlobalSearchScope
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.intellij.util.indexing.FindSymbolParameters
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * Cross-file resolution, soft references, Go-to contributors, and the
 * override/implement gutter — the resolve-layer additions of the polish wave.
 */
class JuxResolveAndNavTest : BasePlatformTestCase() {

    fun testReferenceIsSoft() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A { public void go() { unknownThing(); } }
            """.trimIndent(),
        )
        val offset = myFixture.file.text.indexOf("unknownThing")
        val ref = myFixture.file.findReferenceAt(offset)
        assertNotNull("identifier should carry a reference", ref)
        assertTrue("the in-file resolver must be soft", ref!!.isSoft)
    }

    fun testCrossFileTypeResolution() {
        myFixture.addFileToProject("beast.jux", "package demo;\npublic class Beast {}\n")
        myFixture.configureByText(
            "user.jux",
            """
            package demo;
            public class User { public void pet(Beast b) {} }
            """.trimIndent(),
        )
        val offset = myFixture.file.text.indexOf("Beast")
        val resolved = myFixture.file.findReferenceAt(offset)?.resolve()
        assertNotNull("Beast should resolve cross-file via JuxTypeIndex", resolved)
        assertEquals("Beast", (resolved as JuxTypeDeclaration).name)
    }

    fun testGotoClassContributorFindsTypes() {
        myFixture.addFileToProject("beast.jux", "package demo;\npublic class Beast {}\n")
        myFixture.configureByText("other.jux", "package demo;\npublic class Other {}\n")

        val names = ArrayList<String>()
        JuxGotoClassContributor().processNames(
            { names.add(it) },
            GlobalSearchScope.projectScope(project),
            null,
        )
        assertContainsElements(names, "Beast", "Other")

        val items = ArrayList<Any>()
        JuxGotoClassContributor().processElementsWithName(
            "Beast",
            { items.add(it) },
            FindSymbolParameters.simple(project, false),
        )
        assertSize(1, items)
    }

    fun testGotoSymbolContributorFindsMembers() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Box { public int size; public void open() {} }
            """.trimIndent(),
        )
        val names = ArrayList<String>()
        JuxGotoSymbolContributor().processNames(
            { names.add(it) },
            GlobalSearchScope.projectScope(project),
            null,
        )
        assertContainsElements(names, "Box", "size", "open")
    }

    fun testOverrideGutterAppearsOnOverridingMethod() {
        myFixture.addFileToProject(
            "animal.jux",
            """
            package demo;
            public class Animal { public void speak() {} }
            """.trimIndent(),
        )
        myFixture.configureByText(
            "dog.jux",
            """
            package demo;
            public class Dog extends Animal {
                @override
                public void spea<caret>k() {}
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val gutters = myFixture.findGuttersAtCaret()
        assertTrue(
            "expected an overrides/implements gutter on Dog.speak",
            gutters.any { it.tooltipText?.contains("Animal") == true },
        )
    }
}
