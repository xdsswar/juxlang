package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Gaps in the IDE-side fallback completion — the path that runs on Community
 * IDEs and whenever `juxc-lsp` is not serving. (`JuxLspState.isServing` is
 * hardwired `false` under the test fixture, so every test here exercises the
 * fallback by construction.)
 *
 * Each case is something the LSP already gets right, so the fallback being
 * wrong is a difference the user feels only when the server is gone — the worst
 * time to discover it.
 */
class JuxFallbackCompletionGapsTest : BasePlatformTestCase() {

    private fun offered(code: String, name: String = "a.jux"): List<String> {
        myFixture.configureByText(name, code)
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    // ---- completion must not fire inside comments ---------------------------

    // An EMPTY prefix is the discriminator here: a missing guard shows the whole
    // keyword + declaration list, whereas a typed prefix can auto-insert its one
    // match and return an empty list for entirely the wrong reason.

    fun testNoCompletionInsideLineComment() {
        val o = offered("void main() {\n    // <caret>\n}")
        assertTrue("a line comment is prose, not code: $o", o.isEmpty())
    }

    fun testNoCompletionInsideBlockComment() {
        val o = offered("void main() {\n    /* <caret> */\n}")
        assertTrue("a block comment is prose, not code: $o", o.isEmpty())
    }

    fun testNoCompletionInsideDocComment() {
        val o = offered("/**\n * <caret>\n */\npublic class C { }")
        assertTrue("a doc comment is prose, not code: $o", o.isEmpty())
    }

    /** The guard must not swallow the real code around a comment. */
    fun testCompletionStillWorksAroundComments() {
        val o = offered("void main() {\n    // note\n    <caret>\n}")
        assertTrue("code after a comment line still completes: $o", o.contains("return"))
    }

    // ---- `const` members after a dot ---------------------------------------

    // `const` alone declares an INSTANCE member in Jux — the compiler rejects
    // `Limits.lo` with E0412 and accepts `l.lo`. Only `static const` is reached
    // through the type. Both were missing from the member list entirely.

    fun testStaticConstCompletesOnTheType() {
        val o = offered(
            """
            public class Limits {
                public static const int MAX = 9;
                public static int of() { return 1; }
            }
            void main() { Limits.<caret> }
            """.trimIndent(),
        )
        assertTrue("a static const is reached through the type: $o", o.contains("MAX"))
        assertTrue("static methods still offered: $o", o.contains("of"))
    }

    fun testInstanceConstCompletesOnAnInstance() {
        val o = offered(
            """
            public class Limits {
                public const int lo = 1;
                public static const int MAX = 9;
            }
            void main() { Limits l = new Limits(); l.<caret> }
            """.trimIndent(),
        )
        assertTrue("a plain const is an instance member: $o", o.contains("lo"))
        assertFalse("a static const does not belong on an instance: $o", o.contains("MAX"))
    }

    // ---- import paths -------------------------------------------------------

    fun testImportPathOffersTypesInThatPackage() {
        myFixture.addFileToProject(
            "pkg/Thing.jux",
            "package demo.model;\npublic class Thing { }\npublic class Other { }\n",
        )
        val o = offered("import demo.model.<caret>", "b.jux")
        assertTrue("types of the named package: $o", o.containsAll(listOf("Thing", "Other")))
    }

    fun testImportPathOffersDeeperPackageSegments() {
        myFixture.addFileToProject(
            "pkg/Thing.jux",
            "package demo.model.core;\npublic class Thing { }\n",
        )
        val o = offered("import demo.<caret>", "c.jux")
        assertTrue("the next package segment: $o", o.contains("model"))
    }

    /** An import path that names nothing must stay empty, not fall back to noise. */
    fun testUnknownImportPathOffersNothing() {
        val o = offered("import nowhere.at.all.<caret>", "d.jux")
        assertTrue("no keywords or locals in an import path: $o", o.isEmpty())
    }
}
