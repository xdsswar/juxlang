package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Which TYPES another package may see.
 *
 * §4.4: `public` is visible anywhere, a no-modifier declaration is "visible
 * within this package only", `internal` within the module. The cross-file type
 * walk offered every declaration in the project regardless — so a package's
 * internal helper showed up in another package's popup, and accepting it wrote
 * an `import` for a name that package has no business naming.
 *
 * Encapsulation is only real if the tools respect it; a modifier the editor
 * ignores is a comment.
 */
class JuxTypeVisibilityTest : BasePlatformTestCase() {

    private fun offered(code: String): List<String> {
        myFixture.configureByText("use.jux", code)
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    private fun addOtherPackage() {
        myFixture.addFileToProject(
            "a/A.jux",
            """
            package demo.a;
            public class Exported { }
            class PackageOnly { }
            internal class ModuleOnly { }
            """.trimIndent(),
        )
    }

    fun testOnlyPublicTypesCrossAPackageBoundary() {
        addOtherPackage()
        val o = offered("package demo.b;\nvoid main() { <caret> }")
        assertTrue("a public type crosses: $o", o.contains("Exported"))
        assertFalse("a package-private type does not: $o", o.contains("PackageOnly"))
    }

    fun testPackagePrivateTypesAreOfferedInsideTheirOwnPackage() {
        addOtherPackage()
        val o = offered("package demo.a;\nvoid main() { <caret> }")
        assertTrue("its own package sees it: $o", o.contains("PackageOnly"))
        assertTrue("and the public one: $o", o.contains("Exported"))
    }

    /**
     * `internal` is module-scoped, and the IDE has no reliable module identity
     * for a Jux project, so it is treated as visible rather than guessed at.
     * Hiding a name that is legal is the worse error of the two.
     */
    fun testInternalTypesStayVisible() {
        addOtherPackage()
        val o = offered("package demo.b;\nvoid main() { <caret> }")
        assertTrue("internal is not guessed at: $o", o.contains("ModuleOnly"))
    }

    /** A file with no package declaration is its own scope; nothing is hidden from it. */
    fun testNoPackageFileSeesEverything() {
        addOtherPackage()
        val o = offered("void main() { <caret> }")
        assertTrue("the public type: $o", o.contains("Exported"))
    }
}
