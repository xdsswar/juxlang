package dev.jux.intellij.resolve

import com.intellij.psi.PsiFile
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.completion.JuxAutoImport

/**
 * Which declaration a bare type name resolves to when several files declare
 * it.
 *
 * Two types sharing a bare name is not a mistake -- `Animal` in `zoo.wild`
 * and `Animal` in `farm.stock` are different types and a project may hold
 * both. What is a mistake is picking between them at random, which is what
 * "return the first declaration the project-wide lookup met" amounts to. It
 * surfaced as a real false error on a corpus example:
 *
 *     polish_app.jux   Class 'Dog' doesn't implement abstract method(s):
 *                      'Animal.sound' (E0429)
 *
 * on a file whose second line reads `import poll.lib.Animal;`, naming the
 * concrete one. The answer was in the file and nothing read it.
 */
class JuxTypeResolutionTest : BasePlatformTestCase() {

    /** Two `Animal`s: one abstract, one concrete, in different packages. */
    private fun twoAnimals() {
        myFixture.addFileToProject(
            "wild.jux",
            """
            package zoo.wild;

            public abstract class Animal {
                public abstract String sound();
            }
            """.trimIndent(),
        )
        myFixture.addFileToProject(
            "stock.jux",
            """
            package farm.stock;

            public class Animal {
                public String speak() { return "moo"; }
            }
            """.trimIndent(),
        )
    }

    private fun resolveAnimalIn(file: PsiFile): JuxTypeDeclarationOrNull =
        JuxTypeIndex.findType(file.firstChild ?: file, "Animal")

    fun testAnExplicitImportDecidesBetweenSameNamedTypes() {
        twoAnimals()
        val user = myFixture.configureByText(
            "user.jux",
            """
            package app;

            import farm.stock.Animal;

            public class Dog extends Animal { }
            """.trimIndent(),
        )

        val resolved = resolveAnimalIn(user)
        assertNotNull("Animal resolves", resolved)
        assertEquals(
            "the imported package wins",
            "farm.stock",
            JuxAutoImport.packageOf(resolved!!),
        )
    }

    fun testTheOtherImportResolvesTheOtherWay() {
        // The same project, the other import: if this did not flip, the first
        // test would pass on a lookup that ignores imports entirely.
        twoAnimals()
        val user = myFixture.configureByText(
            "user.jux",
            """
            package app;

            import zoo.wild.Animal;

            public class Lion extends Animal { }
            """.trimIndent(),
        )

        val resolved = resolveAnimalIn(user)
        assertNotNull("Animal resolves", resolved)
        assertEquals("the imported package wins", "zoo.wild", JuxAutoImport.packageOf(resolved!!))
    }

    fun testASamePackageSiblingNeedsNoImport() {
        twoAnimals()
        val user = myFixture.configureByText(
            "sibling.jux",
            """
            package farm.stock;

            public class Cow extends Animal { }
            """.trimIndent(),
        )

        val resolved = resolveAnimalIn(user)
        assertNotNull("Animal resolves", resolved)
        assertEquals(
            "a sibling in the same package beats an unimported stranger",
            "farm.stock",
            JuxAutoImport.packageOf(resolved!!),
        )
    }

    fun testTheFilesOwnDeclarationWins() {
        twoAnimals()
        val user = myFixture.configureByText(
            "own.jux",
            """
            package app;

            import zoo.wild.Animal;

            public class Animal { }

            public class Pet extends Animal { }
            """.trimIndent(),
        )

        val resolved = resolveAnimalIn(user)
        assertNotNull("Animal resolves", resolved)
        assertEquals(
            "a declaration in this very file outranks any import",
            "app",
            JuxAutoImport.packageOf(resolved!!),
        )
    }
}

/** Alias so the assertions above read as intent rather than as nullability. */
private typealias JuxTypeDeclarationOrNull = dev.jux.intellij.psi.JuxTypeDeclaration?
