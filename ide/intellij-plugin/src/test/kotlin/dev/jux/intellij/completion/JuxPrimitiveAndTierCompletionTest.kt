package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.highlight.JuxKeywords

/**
 * Two things the popup owes a type position: EVERY built-in type name, and an
 * order that puts the likeliest one on top.
 *
 * The primitive set is read from [JuxKeywords.PRIMITIVES], which the build
 * generates from `grammar/jux-tokens.json`, itself emitted by the compiler's
 * lexer. Asserting against that set rather than a list retyped here is the
 * point: a primitive the language gains is covered by this test the moment
 * the JSON is regenerated, and a primitive the editor forgets fails it. Both
 * spellings of the string type are in the set, and both are real code:
 * `string st = "Some";` is as valid as `String st = "Some";`.
 *
 * The ordering tests assert the ORDER, not just membership. The tiers, most
 * relevant first: this file's own declarations and type parameters, this
 * file's package (no `import` needed), the primitives, what the file already
 * imports, the auto-prelude, and last what accepting would have to import.
 */
class JuxPrimitiveAndTierCompletionTest : BasePlatformTestCase() {

    private fun names(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    /**
     * True when completing [code] offers [expected].
     *
     * A prefix that matches exactly ONE item is completed outright: the
     * platform inserts it and `completeBasic` returns null rather than a
     * list. That is the strongest possible form of "it was offered", so the
     * check falls back to reading the word the fixture just wrote. Without
     * this, the narrowest and most convincing cases (`Vec<stri‸>` with only
     * `string` left) would read as failures.
     */
    private fun offers(code: String, expected: String): Boolean {
        myFixture.configureByText("a.jux", code.trimIndent())
        val items = myFixture.completeBasic() ?: return myFixture.file.text.contains(expected)
        return items.any { it.lookupString == expected }
    }

    /** The index of [name] in [names], failing the test when it is absent. */
    private fun indexOf(names: List<String>, name: String): Int {
        val i = names.indexOf(name)
        assertTrue("'$name' is offered at all: $names", i >= 0)
        return i
    }

    // ---- every primitive, everywhere a type may be written ------------------

    fun testEveryPrimitiveIsOfferedWhereALocalMayStart() {
        val o = names(
            """
            void main() {
                <caret>
            }
            """,
        )
        val missing = JuxKeywords.PRIMITIVES.filter { it !in o }
        assertTrue("every primitive is offered, missing $missing", missing.isEmpty())
    }

    fun testBothSpellingsOfTheStringPrimitiveAreOffered() {
        val o = names(
            """
            void main() {
                <caret>
            }
            """,
        )
        assertTrue("`String` is offered: $o", "String" in o)
        assertTrue("`string` is offered: $o", "string" in o)
    }

    fun testEveryPrimitiveIsOfferedInAParameterList() {
        val o = names("void show(<caret>) {}")
        val missing = JuxKeywords.PRIMITIVES.filter { it !in o }
        assertTrue("every primitive is offered in a parameter list, missing $missing", missing.isEmpty())
    }

    fun testEveryPrimitiveIsOfferedWhereAMemberMayStart() {
        val o = names(
            """
            public class Config {
                <caret>
            }
            """,
        )
        val missing = JuxKeywords.PRIMITIVES.filter { it !in o }
        assertTrue("every primitive is offered as a field type, missing $missing", missing.isEmpty())
    }

    fun testPrimitivesAreOfferedAfterNew() {
        // `new int[8]` is an array creation, so a primitive is legal here.
        val o = names(
            """
            void main() {
                var values = new <caret>
            }
            """,
        )
        assertTrue("`int` after `new`: $o", "int" in o)
    }

    fun testPrimitivesAreOfferedInAGenericArgument() {
        assertTrue("`string` as a type argument", offers("void show(Vec<stri<caret>> names) {}", "string"))
    }

    fun testPrimitivesAreOfferedAsACastTarget() {
        assertTrue(
            "`long` after `as`",
            offers(
                """
                void main(any x) {
                    var n = x as lo<caret>
                }
                """,
                "long",
            ),
        )
    }

    fun testPrimitivesAreOfferedInATypeTest() {
        assertTrue(
            "`double` after `=>`",
            offers(
                """
                void main(any x) {
                    if (x => dou<caret>) {}
                }
                """,
                "double",
            ),
        )
    }

    fun testNullablePrimitiveCompletesToTheBareName() {
        // `T?` is well-formed for every type, primitives included (ERRATA E5):
        // the item inserts the type, and the `?` is the user's next keystroke.
        myFixture.configureByText(
            "a.jux",
            """
            void main() {
                stri<caret>
            }
            """.trimIndent(),
        )
        myFixture.completeBasic()
        myFixture.type("\n")
        assertTrue("the bare name goes in: ${myFixture.file.text}", myFixture.file.text.contains("string"))
    }

    // ---- ranking tiers ------------------------------------------------------

    fun testFileTypesOutrankSamePackageOutranksImportedOutranksTheRest() {
        myFixture.addFileToProject("Neighbour.jux", "package app;\npublic class ZedNeighbour {}\n")
        myFixture.addFileToProject("Imported.jux", "package lib;\npublic class ZedImported {}\n")
        myFixture.addFileToProject("Far.jux", "package far;\npublic class ZedFar {}\n")
        myFixture.configureByText(
            "a.jux",
            """
            package app;

            import lib.ZedImported;

            public class ZedHere {}

            void main() {
                Zed<caret>
            }
            """.trimIndent(),
        )
        val o = myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
        val here = indexOf(o, "ZedHere")
        val neighbour = indexOf(o, "ZedNeighbour")
        val imported = indexOf(o, "ZedImported")
        val far = indexOf(o, "ZedFar")
        assertTrue("this file before its package: $o", here < neighbour)
        assertTrue("this package before an import: $o", neighbour < imported)
        assertTrue("an import before a name that needs one: $o", imported < far)
    }

    fun testATypeInThisFileOutranksAPrimitiveWithTheSamePrefix() {
        val o = names(
            """
            public class Strand {}
            void main() {
                Str<caret>
            }
            """,
        )
        assertTrue("the file's own type first: $o", indexOf(o, "Strand") < indexOf(o, "String"))
    }

    fun testATypeParameterOutranksAPreludeType() {
        val o = names(
            """
            public class Box<Sample> {
                public void put(S<caret>) {}
            }
            """,
        )
        // A type parameter is a name written a line above the caret; `Stream`
        // is a prelude name that every file can reach. The nearer one wins.
        assertTrue("a type parameter first: $o", indexOf(o, "Sample") < indexOf(o, "Stream"))
        assertTrue("and above a primitive too: $o", indexOf(o, "Sample") < indexOf(o, "String"))
    }
}
