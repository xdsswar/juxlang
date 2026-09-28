package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * E0303 for a simple name two wildcard imports bring for different types
 * (ERRATA E132, `tests/ui/ambiguous_wildcard_import`): an error where the name
 * is USED, never for the imports themselves.
 */
class JuxAmbiguousImportInspectionTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(JuxAmbiguousImportInspection())
        myFixture.addFileToProject("alpha/Widget.jux", "package alpha;\n\npublic class Widget { public int size() { return 1; } }\n")
        myFixture.addFileToProject("beta/Widget.jux", "package beta;\n\npublic class Widget { public int size() { return 2; } }\n")
        myFixture.addFileToProject("beta/Gauge.jux", "package beta;\n\npublic class Gauge { public int level() { return 7; } }\n")
    }

    private fun e0303(code: String): List<String> {
        myFixture.configureByText("app.jux", code.trimIndent())
        return myFixture.doHighlighting().mapNotNull { it.description }.filter { it.contains("E0303") }
    }

    fun testAUseOfTheSharedNameIsAmbiguous() {
        val d = e0303(
            """
            package app;

            import alpha.*;
            import beta.*;

            public void main() {
                print(new Gauge().level());
                Widget w = new Widget();
                print(w.size());
            }
            """,
        )
        // The declared type and the `new`, as the compiler reports them.
        assertEquals(d.toString(), 2, d.size)
        assertTrue(d.first(), d.first().contains("both 'alpha.Widget' and 'beta.Widget'"))
        assertTrue(d.first(), d.first().contains("'import beta.Widget;'"))
    }

    fun testUnusedAmbiguityIsNotAnError() {
        assertEquals(
            emptyList<String>(),
            e0303(
                """
                package app;

                import alpha.*;
                import beta.*;

                public void main() {
                    print(new Gauge().level());
                }
                """,
            ),
        )
    }

    fun testASingleTypeImportSettlesIt() {
        assertEquals(
            emptyList<String>(),
            e0303(
                """
                package app;

                import alpha.*;
                import beta.*;
                import beta.Widget;

                public void main() {
                    Widget w = new Widget();
                }
                """,
            ),
        )
    }

    fun testAStaticAccessHeadIsAUse() {
        myFixture.addFileToProject("alpha/Tools.jux", "package alpha;\n\npublic class Tools { public static int one() { return 1; } }\n")
        myFixture.addFileToProject("beta/Tools.jux", "package beta;\n\npublic class Tools { public static int one() { return 2; } }\n")
        val d = e0303(
            """
            package app;

            import alpha.*;
            import beta.*;

            public void main() {
                print(Tools.one());
            }
            """,
        )
        assertEquals(d.toString(), 1, d.size)
    }

    fun testTwoSingleTypeImportsOfOneNameConflict() {
        val d = e0303(
            """
            package app;

            import alpha.Widget;
            import beta.Widget;
            import beta.Gauge;
            import beta.Gauge;
            """,
        )
        assertEquals(d.toString(), 1, d.size)
        assertTrue(d.single(), d.single().contains("imported from both 'alpha.Widget' and 'beta.Widget'"))
    }

    fun testAnAliasClashingWithAnImportConflicts() {
        val d = e0303(
            """
            package app;

            import alpha.Widget;
            import beta.Gauge as Widget;
            """,
        )
        assertEquals(d.toString(), 1, d.size)
    }

    fun testTheFixImportsTheChosenType() {
        myFixture.configureByText(
            "app.jux",
            """
            package app;

            import alpha.*;
            import beta.*;

            public void main() {
                Wid<caret>get w = null;
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        myFixture.launchAction(myFixture.findSingleIntention("Import 'beta.Widget'"))
        assertTrue(myFixture.file.text, myFixture.file.text.contains("import beta.*;\nimport beta.Widget;"))
        assertTrue(myFixture.doHighlighting().none { it.description?.contains("E0303") == true })
    }
}
