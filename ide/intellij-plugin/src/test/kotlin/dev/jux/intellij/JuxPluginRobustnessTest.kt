package dev.jux.intellij

import com.intellij.psi.PsiDocumentManager
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * End-to-end resilience + freshness guarantees the editor experience leans on:
 *
 *  - highlighting/annotators never throw on malformed, half-typed source — they
 *    must degrade, not crash (lexer-driven syntax coloring works token-by-token
 *    regardless of parse state);
 *  - declared types are RE-discovered as files are edited or added, so
 *    completion / Go-to / coloring track the project live (the [JuxTypeIndex]
 *    per-file cache is keyed on modification stamp and must invalidate).
 */
class JuxPluginRobustnessTest : BasePlatformTestCase() {

    // ---- "no matter what": highlighting survives broken input -----------------

    fun testHighlightingDoesNotThrowOnMalformedSource() {
        // Unbalanced braces/parens, an unterminated interpolation hole, missing
        // semicolons, dangling clauses — a worst-case mid-edit buffer.
        val broken = """
            package demo
            public class A <T extends {
                private String name =
                public void f(int x, {
                    var s = ${'$'}"hi ${'$'}{ name + ${'$'}{ x
                    return
                }
            """.trimIndent()
        myFixture.configureByText("a.jux", broken)
        // The whole highlighting pipeline (lexer colors + annotators +
        // inspections) must complete without throwing.
        val infos = myFixture.doHighlighting()
        assertNotNull(infos)
    }

    fun testInterpolationColoringRobustOnUnterminatedHole() {
        // `${ name` never closes and the string never closes — the annotator
        // must still color the variable it can see and not blow up.
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {
                public void f(String name) {
                    var s = ${'$'}"hi ${'$'}{ name
                }
            }
            """.trimIndent(),
        )
        val keys = myFixture.doHighlighting()
            .filter { it.text == "name" }
            .mapNotNull { it.forcedTextAttributesKey?.externalName }
        assertTrue("variable in an unterminated hole still colored: $keys", keys.any { it == "JUX_INTERPOLATED_VARIABLE" })
    }

    // ---- "discover and rediscover on changes" --------------------------------

    fun testTypeIndexRediscoversTypeAddedByEdit() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {}
            <caret>
            """.trimIndent(),
        )
        assertNull("Beast not present yet", JuxTypeIndex.findType(project, "Beast"))

        // Type a brand-new declaration; the index must see it on the next query.
        myFixture.type("public class Beast {}")
        PsiDocumentManager.getInstance(project).commitAllDocuments()

        assertNotNull(
            "edited-in type must be rediscovered (cache invalidated on change)",
            JuxTypeIndex.findType(project, "Beast"),
        )
    }

    fun testTypeIndexRediscoversNewlyAddedFile() {
        myFixture.configureByText("App.jux", "package app;\npublic class App {}")
        assertNull("Gadget absent before its file exists", JuxTypeIndex.findType(project, "Gadget"))

        myFixture.addFileToProject("lib/Gadget.jux", "package lib;\npublic class Gadget {}")
        assertNotNull(
            "type from a newly-added file is discovered project-wide",
            JuxTypeIndex.findType(project, "Gadget"),
        )
    }

    fun testCompletionReflectsLocalsTypedAfterConfigure() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {
                public void go() {
                    <caret>
                }
            }
            """.trimIndent(),
        )
        // Introduce a local, then complete against its prefix — the contributor
        // must walk the freshly-parsed scope, not a stale snapshot.
        myFixture.type("var freshLocal = 1;\nfresh")
        myFixture.completeBasic()
        val items = myFixture.lookupElementStrings ?: emptyList()
        // Either the unique match auto-inserted, or it's offered.
        assertTrue(
            "edited-in local is visible to completion: $items / ${myFixture.file.text}",
            items.contains("freshLocal") || myFixture.file.text.contains("freshLocal = 1;\n        freshLocal"),
        )
    }
}
