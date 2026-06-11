package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The three native inspections + their quick-fixes: unused/duplicate imports,
 * unused locals/params/private fields (shadowing-aware), missing `@override`.
 */
class JuxInspectionsTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxUnusedImportInspection(),
            JuxUnusedLocalSymbolInspection(),
            JuxMissingOverrideInspection(),
        )
    }

    private fun highlightDescriptions(code: String, fileName: String = "a.jux"): List<String> {
        myFixture.configureByText(fileName, code)
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    // ---- unused imports -----------------------------------------------------

    fun testUnusedImportFlaggedAndUsedImportNot() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            import rust.std.collections.Map;
            import rust.std.collections.Set;

            public class A {
                public Map<int, int> m;
            }
            """.trimIndent(),
        )
        assertTrue("Set import should be flagged", descriptions.any { it == "Unused import" })
        // Exactly one unused (Map is used).
        assertEquals(1, descriptions.count { it == "Unused import" })
    }

    fun testDuplicateImportFlagged() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            import rust.std.collections.Map;
            import rust.std.collections.Map;

            public class A { public Map<int, int> m; }
            """.trimIndent(),
        )
        assertTrue(descriptions.any { it == "Duplicate import" })
    }

    fun testRemoveImportQuickFix() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            import rust.std.collections.Unu<caret>sed;

            public class A {}
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val fix = myFixture.findSingleIntention("Remove import")
        myFixture.launchAction(fix)
        assertFalse("import should be gone", myFixture.file.text.contains("import"))
    }

    // ---- unused locals/params/fields ---------------------------------------

    fun testUnusedLocalFlaggedUsedLocalNot() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class A {
                public int go() {
                    var used = 1;
                    var unused = 2;
                    return used;
                }
            }
            """.trimIndent(),
        )
        assertTrue(descriptions.any { it == "Variable 'unused' is never used" })
        assertFalse(descriptions.any { it.contains("'used'") })
    }

    fun testUnusedParameterFlaggedButOverrideParamSkipped() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class Base { public void f(int a) { print(a); } }
            public class A extends Base {
                @override
                public void f(int a) {}
                public void g(int ghost) {}
            }
            """.trimIndent(),
        )
        assertTrue(descriptions.any { it == "Parameter 'ghost' is never used" })
        assertFalse("override params are exempt", descriptions.any { it == "Parameter 'a' is never used" })
    }

    fun testUnusedPrivateFieldFlaggedPublicSkipped() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class A {
                private int hidden;
                public int visible;
            }
            """.trimIndent(),
        )
        assertTrue(descriptions.any { it == "Field 'hidden' is never used" })
        assertFalse("non-private fields are cross-file API", descriptions.any { it.contains("'visible'") })
    }

    /**
     * Pass-2 regression: names used ONLY inside `$"…${…}…"` interpolation
     * holes (one lexer token — invisible to the resolver) must count as
     * usages, for both the unused-import and unused-local inspections. The
     * quick-fixes DELETE code, so a false positive here broke builds.
     */
    fun testInterpolationHoleUsageSuppressesUnusedFlags() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            import rust.std.collections.Map;

            public class A {
                public void go() {
                    int count = 3;
                    print(${'$'}"have ${'$'}{count} in ${'$'}{Map.of()}");
                }
            }
            """.trimIndent(),
        )
        assertFalse(
            "import used in a hole must survive: $descriptions",
            descriptions.any { it == "Unused import" },
        )
        assertFalse(
            "local used in a hole must survive: $descriptions",
            descriptions.any { it.contains("'count'") },
        )
    }

    /** Names used only in switch-case patterns are resolver-blind — exempt. */
    fun testPatternUsageSuppressesUnusedField() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class A {
                private int MAX = 9;
                public int pick(int x) {
                    return switch (x) {
                        case MAX -> 1;
                        default -> 0;
                    };
                }
            }
            """.trimIndent(),
        )
        assertFalse(
            "field used in a case pattern must survive: $descriptions",
            descriptions.any { it.contains("'MAX'") },
        )
    }

    // ---- missing @override --------------------------------------------------

    fun testMissingOverrideFlaggedAndFixAddsAnnotation() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Animal { public void speak() { print("?"); } }
            public class Dog extends Animal {
                public void spe<caret>ak() { print("woof"); }
            }
            """.trimIndent(),
        )
        val descriptions = myFixture.doHighlighting().mapNotNull { it.description }
        assertTrue(descriptions.any { it.contains("not annotated @override") })

        val fix = myFixture.findSingleIntention("Add @override")
        myFixture.launchAction(fix)
        val text = myFixture.file.text
        assertTrue("@override inserted above the method", text.contains("@override\n    public void speak()"))
    }

    fun testAnnotatedOverrideNotFlagged() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class Animal { public void speak() {} }
            public class Dog extends Animal {
                @Override
                public void speak() {}
            }
            """.trimIndent(),
        )
        assertFalse(
            "case-insensitive @Override satisfies the check",
            descriptions.any { it.contains("not annotated @override") },
        )
    }
}
