package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.lsp.JuxLspState

/**
 * What the plugin contributes while `juxc-lsp` IS serving.
 *
 * This is the configuration most users are in: the server starts on the first
 * `.jux` file opened. Under the hybrid engine the plugin owns completion in
 * BOTH configurations -- its PSI type engine follows chains, generics and
 * imports, and renders items with Java's insert handlers -- while the native
 * client's own LSP completion is switched off (`JuxLspDescriptor`), so the
 * popup is never doubled. The server keeps diagnostics and hover.
 *
 * These tests pin that nothing stands down any more: the same names come out
 * serving or not.
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

    fun testCompletionIsThePluginsWhileTheServerServes() {
        val code = "public class Model { }\nvoid main() { <caret> }"
        assertTrue("offered without a server", offeredWithoutServer(code).contains("Model"))
        assertTrue("and the same while serving", offeredWhileServing(code).contains("Model"))
    }

    fun testMemberCompletionIsThePluginsWhileTheServerServes() {
        val code = """
            public class Leaf { public int size() { return 1; } }
            void main() { Leaf l = new Leaf(); l.<caret> }
        """.trimIndent()
        assertTrue("members without a server", offeredWithoutServer(code).contains("size"))
        assertTrue("and while serving", offeredWhileServing(code).contains("size"))
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
