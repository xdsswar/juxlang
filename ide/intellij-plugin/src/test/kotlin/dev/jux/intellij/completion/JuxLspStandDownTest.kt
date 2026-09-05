package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.lsp.JuxLspState

/**
 * What the plugin contributes while `juxc-lsp` IS serving.
 *
 * This is the configuration most users are in — the server starts on the first
 * `.jux` file opened — and it was the one configuration never tested: the
 * fixture starts no server, so `isServing` short-circuited to `false` and every
 * other test in this package exercised the fallback.
 *
 * The contract has three parts, and the value here is that it is a CONTRACT
 * rather than an accident of ordering in one function:
 *
 * 1. the plugin stands down for everything the server does better,
 * 2. except inside a `$"…${ }…"` interpolation hole, which the server sees as
 *    one opaque string token and never completes,
 * 3. and except the §P observable surface, which the server does not model.
 *
 * Both exceptions are placed BEFORE the stand-down in the dispatch, so a
 * refactor that reorders it would silently delete two features.
 */
class JuxLspStandDownTest : BasePlatformTestCase() {

    /** Keeps Kotlin string interpolation out of the Jux snippets. */
    private val D = '$'

    override fun tearDown() {
        try {
            JuxLspState.servingOverride = null
        } finally {
            super.tearDown()
        }
    }

    private fun offeredWhileServing(code: String): List<String> {
        JuxLspState.servingOverride = true
        myFixture.configureByText("a.jux", code)
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    private fun offeredWithoutServer(code: String): List<String> {
        JuxLspState.servingOverride = false
        myFixture.configureByText("b.jux", code)
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    fun testTheFallbackStandsDownWhileTheServerServes() {
        val code = "public class Model { }\nvoid main() { <caret> }"
        assertTrue("the fallback offers names on its own", offeredWithoutServer(code).isNotEmpty())
        assertTrue(
            "the server supplies a better list; the plugin must not duplicate it",
            offeredWhileServing(code).isEmpty(),
        )
    }

    fun testMemberCompletionStandsDownToo() {
        val code = """
            public class Leaf { public int size() { return 1; } }
            void main() { Leaf l = new Leaf(); l.<caret> }
        """.trimIndent()
        assertTrue("the fallback resolves it", offeredWithoutServer(code).contains("size"))
        assertTrue("the server owns members", offeredWhileServing(code).isEmpty())
    }

    /**
     * The server sees `$"…"` as one opaque token, so it never completes inside
     * a hole. The plugin therefore owns that surface, serving or not.
     */
    fun testInterpolationHolesAreOfferedEvenWhileServing() {
        // A caret with an EMPTY prefix inside the hole, matching the shape the
        // fallback's own tests use; a typed prefix can auto-insert its single
        // match and return an empty list for the wrong reason.
        val code = """
            public class A {
                private int width;
                public void go(int param) {
                    var local = 1;
                    var s = ${D}"x=${D}{ <caret> }";
                }
            }
        """.trimIndent()
        val without = offeredWithoutServer(code)
        assertTrue("the fallback offers them: $without", without.contains("local"))
        val serving = offeredWhileServing(code)
        assertTrue(
            "an interpolation hole is the plugin's, server or no server: $serving",
            serving.contains("local"),
        )
    }

    /**
     * The §P observable surface is not modelled by the server at all, so
     * standing down for it would delete the feature outright.
     */
    fun testPropertySurfaceIsOfferedEvenWhileServing() {
        val code = """
            public class Model { public int Value { get; set; } = 0; }
            void main() { Model m = new Model(); m.Value.<caret> }
        """.trimIndent()
        val offered = offeredWhileServing(code)
        assertTrue("the §P surface is plugin-owned: $offered", offered.contains("observers"))
        assertTrue("including the binding ops: $offered", offered.contains("bind"))
    }

    /** A comment is prose in both configurations. */
    fun testCommentsStaySilentInBothConfigurations() {
        val code = "void main() {\n    // <caret>\n}"
        assertTrue(offeredWithoutServer(code).isEmpty())
        assertTrue(offeredWhileServing(code).isEmpty())
    }
}
