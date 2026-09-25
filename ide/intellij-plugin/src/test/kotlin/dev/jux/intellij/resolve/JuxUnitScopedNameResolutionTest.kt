package dev.jux.intellij.resolve

import com.intellij.psi.PsiFile
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * Bare-name resolution is **unit-scoped**, and the standard library has its own
 * realm (`JUX-MISSING-DEFS-ADDENDUM.md` §M.16, ERRATA E96 and E102).
 *
 * The ladder the compiler walks, and so the one the editor has to walk: a
 * generic parameter in scope, then a declaration the unit itself makes or one an
 * `import` in the unit binds or one the unit's own package makes, then a nested
 * type of the enclosing type, then the implicit prelude (`jux.std.*`,
 * `rust.std`). Anything else is unresolved.
 *
 * What it cost to get wrong: `examples/stdlib_name_collisions.jux` legally
 * declares its own `interface Iterable { String describe(); }`, because Java
 * lets a program declare `String`, `T`, `Iterator` or `Iterable` and Jux is a
 * Java-shaped language. The IDE holds every file of every program in ONE
 * project, so a workspace-wide lookup of the bare name `Iterable` handed that
 * interface to `class Span implements Iterable<int>` in
 * `examples/iterable_combinators.jux`, a file that never heard of it, and the
 * E0429 inspection demanded `describe()` from it. Red on code that compiles is
 * the single most damaging thing an editor can do, so the corpus test that
 * caught it is the gate, and these are the units of it.
 */
class JuxUnitScopedNameResolutionTest : BasePlatformTestCase() {

    /**
     * A second single-file program in the same source root that redeclares two
     * names the prelude also spells. This is the corpus shape exactly: nothing
     * links it to the file under test but the source root they share.
     */
    private fun addAnotherProgramRedeclaringPreludeNames() {
        myFixture.addFileToProject(
            "collisions.jux",
            """
            interface Iterable { String describe(); }

            class Vec { public int mine = 1; }
            """.trimIndent(),
        )
    }

    private fun resolve(file: PsiFile, name: String): JuxTypeDeclaration? =
        JuxTypeIndex.findType(file.firstChild ?: file, name)

    fun testAnotherFilesRedeclarationDoesNotShadowThePrelude() {
        addAnotherProgramRedeclaringPreludeNames()
        val user = myFixture.configureByText(
            "combinators.jux",
            """
            class Span implements Iterable<int> {
                public Iterator<int> iterator() { return null; }
            }
            """.trimIndent(),
        )

        val resolved = resolve(user, "Iterable")
        assertNotNull("Iterable resolves", resolved)
        assertEquals(
            "a prelude name is shadowed by the unit, not by the workspace",
            "jux.std.collections",
            JuxAutoImport.packageOf(resolved!!),
        )
    }

    fun testTheUnitsOwnRedeclarationOfAPreludeNameStillWins() {
        // The other half: inside the file that WROTE `interface Iterable`, the
        // name means that interface. Without this the fix above would just be
        // "the prelude always wins", which forbids the declaration §M.16.1
        // exists to permit.
        val own = myFixture.configureByText(
            "collisions.jux",
            """
            interface Iterable { String describe(); }

            class Steps implements Iterable {
                public String describe() { return "steps"; }
            }
            """.trimIndent(),
        )

        val resolved = resolve(own, "Iterable")
        assertNotNull("Iterable resolves", resolved)
        assertEquals("the unit's own declaration", "", JuxAutoImport.packageOf(resolved!!))
        assertTrue("and it is the interface, not a class", JuxHierarchy.isInterface(resolved))
    }

    fun testASiblingInANamedPackageStillShadowsThePrelude() {
        // Rung 2 is not gutted: a NAMED package is a deliberate grouping, so a
        // sibling there is the same unit's package and outranks the prelude,
        // exactly as the compiler has it.
        myFixture.addFileToProject(
            "lib.jux",
            """
            package app.core;

            public interface Iterable { String describe(); }
            """.trimIndent(),
        )
        val user = myFixture.configureByText(
            "user.jux",
            """
            package app.core;

            public class Steps implements Iterable { }
            """.trimIndent(),
        )

        val resolved = resolve(user, "Iterable")
        assertNotNull("Iterable resolves", resolved)
        assertEquals("the sibling package wins over the prelude", "app.core", JuxAutoImport.packageOf(resolved!!))
    }

    fun testAnOrdinaryUserNameStillResolvesAcrossFiles() {
        // The narrowing is about names the PRELUDE spells. A plain name that
        // only one other file declares is the ordinary cross-file case, and
        // resolving it is what the project-wide index is for.
        myFixture.addFileToProject("other.jux", "public class Tagged { public int id() { return 1; } }")
        val user = myFixture.configureByText("use.jux", "public class Uses extends Tagged { }")

        assertNotNull("Tagged still resolves from another file", resolve(user, "Tagged"))
    }

    fun testALibraryUnitNeverBindsABareNameToAUserDeclaration() {
        // §M.16.1. A unit of `jux.std.*` resolves against the library realm
        // only. Under the old workspace-wide lookup a program's own `class T`
        // became the `T` of `jux.std.collections.Iterable`'s `(T) -> R`
        // parameters, and `class T { public int v = 1; }` alone produced 60
        // errors pointing inside files the author cannot open (ERRATA E96).
        myFixture.addFileToProject("mine.jux", "class T { public int v = 1; }")
        val library = myFixture.configureByText(
            "libunit.jux",
            """
            package jux.std.collections;

            public interface Sink<T> { void put(T item); }
            """.trimIndent(),
        )

        assertNull(
            "a jux.std unit does not see the program's `class T`",
            resolve(library, "T"),
        )
    }

    fun testAUserPackageNamedJuxIsNotTheLibraryRealm() {
        // §M.16.3: `package jux;` is a legal package declaration, and the realm
        // is named exactly rather than by prefix guesswork. A unit there is an
        // ordinary user unit and sees its own program.
        myFixture.addFileToProject("mine.jux", "public class Widget { }")
        val user = myFixture.configureByText(
            "juxpkg.jux",
            """
            package jux;

            public class Uses extends Widget { }
            """.trimIndent(),
        )

        assertNotNull("`package jux;` is not `jux.std`", resolve(user, "Widget"))
    }

    fun testAQualifiedNameIsNotShadowedByASameNamedUserClass() {
        // §M.16.6 / ERRATA E102. Writing the package out says which type is
        // meant. In the compiler the two halves of that decision disagreed: the
        // SLOT was measured from the written `rust.std.Vec` and the member call
        // from the resolved name's last segment, `Vec`, which a program's own
        // root-package `class Vec` claimed. The editor must not repeat it, so a
        // qualified name that names nothing in that package is UNRESOLVED
        // rather than quietly the user's class.
        myFixture.addFileToProject("mine.jux", "class Vec { public int mine = 1; }")
        val user = myFixture.configureByText("use.jux", "public class Holder { }")

        val resolved = JuxTypeEngine.resolveTypeName(user.firstChild ?: user, "Vec", "rust.std")
        if (resolved != null) {
            assertEquals(
                "a qualified name resolves in the package it names",
                "rust.std",
                JuxAutoImport.packageOf(resolved as JuxTypeDeclaration),
            )
        }
    }

    fun testANestedTypeIsStillReachedThroughItsOuter() {
        // The one qualifier that is not a package (§M.9): E102 keeps the
        // nested-type shadow test a question about a simple name, so
        // `Outer.Inner` must still land on the nested declaration.
        val file = myFixture.configureByText(
            "nest.jux",
            """
            public class Outer {
                public static class Inner { public int v = 1; }
            }
            """.trimIndent(),
        )

        val resolved = JuxTypeEngine.resolveTypeName(file.firstChild ?: file, "Inner", "Outer")
        assertNotNull("Outer.Inner resolves", resolved)
        assertEquals("Inner", (resolved as JuxTypeDeclaration).name)
    }
}
