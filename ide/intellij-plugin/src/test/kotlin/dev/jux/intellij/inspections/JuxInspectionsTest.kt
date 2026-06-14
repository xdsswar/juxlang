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
            JuxPropertyNamingInspection(),
            JuxSetterEarlyReturnInspection(),
            JuxAccessorVisibilityInspection(),
            JuxPropertyNeverObservedInspection(),
            JuxBoundPropertyAssignmentInspection(),
            JuxBindTypeMismatchInspection(),
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

    /** A private property is owned by the §P never-observed inspection (W0971);
     *  it must NOT also be flagged as an "unused field" (no double diagnostic). */
    fun testPrivatePropertyNotFlaggedAsUnusedField() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class A {
                private String Lonely { get; set; } = "";
            }
            """.trimIndent(),
        )
        assertFalse(
            "property must not be flagged as an unused field: $descriptions",
            descriptions.any { it.contains("is never used") },
        )
        assertTrue("property is still covered by W0971", descriptions.any { it.contains("W0971") })
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

    // ---- observable properties (§P.7) ---------------------------------------

    fun testPascalCaseHintFiresAndPascalNamesPass() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class A {
                public String name { get; set; } = "";
                public String Name2 { get; set; } = "";
                private String _backing { get; set; } = "";
            }
            """.trimIndent(),
        )
        assertTrue(descriptions.any { it.contains("'name' should be PascalCase (W0974)") })
        assertFalse(descriptions.any { it.contains("W0974") && it.contains("'Name2'") })
        assertFalse(
            "underscore-prefixed names are exempt",
            descriptions.any { it.contains("W0974") && it.contains("'_backing'") },
        )
    }

    fun testPascalCaseRenameQuickFixUpdatesUsages() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {
                public String na<caret>me { get; set; } = "";
                private final observer<String> obs = (old, now) -> { print(now); };
                public A() {
                    name.observers.attach(obs);
                    name = "x";
                }
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val fix = myFixture.findSingleIntention("Rename property to PascalCase")
        myFixture.launchAction(fix)
        val text = myFixture.file.text
        assertTrue("declaration renamed", text.contains("public String Name { get; set; }"))
        assertTrue("attach site renamed", text.contains("Name.observers.attach(obs);"))
        assertTrue("write site renamed", text.contains("Name = \"x\";"))
    }

    fun testEarlyReturnInSetterFlagged() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class A {
                private String _n = "";
                public String Name {
                    get -> _n;
                    set {
                        if (value == null) return;
                        _n = value;
                    }
                };
                public String Other {
                    get -> _n;
                    set {
                        _n = value;
                        return;
                    }
                };
            }
            """.trimIndent(),
        )
        assertEquals(
            "only the early return is flagged: $descriptions",
            1,
            descriptions.count { it.contains("W0973") },
        )
    }

    fun testSetterVisibilityExceedsGetterFlagged() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class A {
                public String Bad { private get; set; } = "";
                public String Good { get; private set; } = "";
            }
            """.trimIndent(),
        )
        assertEquals(
            "only Bad is flagged: $descriptions",
            1,
            descriptions.count { it.contains("E0972") },
        )
    }

    fun testNeverObservedFiresOnPrivateOnly() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class A {
                private String Lonely { get; set; } = "";
                private String Watched { get; set; } = "";
                public String Api { get; set; } = "";
                private bool Computed { get -> true; };
                private final observer<String> obs = (old, now) -> { print(now); };
                public A() {
                    Watched.observers.attach(obs);
                }
            }
            """.trimIndent(),
        )
        assertTrue(descriptions.any { it.contains("'Lonely' is never observed or bound (W0971)") })
        assertFalse("observed property passes", descriptions.any { it.contains("W0971") && it.contains("'Watched'") })
        assertFalse("public properties are exempt", descriptions.any { it.contains("W0971") && it.contains("'Api'") })
        assertFalse(
            "computed properties are exempt",
            descriptions.any { it.contains("W0971") && it.contains("'Computed'") },
        )
    }

    fun testBoundPropertyAssignmentFlagged() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class Model { public String Name { get; set; } = ""; }
            public class A {
                public String Label { get; set; } = "";
                public void wire(Model m) {
                    Label.bind(m.Name);
                    Label = "direct";
                    m.Name = "fine";
                }
            }
            """.trimIndent(),
        )
        assertEquals("only the bound receiver flags: $descriptions", 1, descriptions.count { it.contains("E0973") })
        assertTrue(descriptions.any { it.contains("'Label' is bound") })
    }

    fun testUnboundPropertyAssignmentNotFlagged() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class Model { public String Name { get; set; } = ""; }
            public class A {
                public String Label { get; set; } = "";
                public void wire(Model m) {
                    Label.bind(m.Name);
                    Label.unbind();
                    Label = "direct";
                }
            }
            """.trimIndent(),
        )
        assertFalse("unbind() silences E0973: $descriptions", descriptions.any { it.contains("E0973") })
    }

    fun testBindTypeMismatchFlaggedWhenBothResolve() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class A {
                public String Name { get; set; } = "";
                public double Value { get; set; } = 0.0;
                public int Count { get; set; } = 0;
                public int Total { get; set; } = 0;
                public void wire() {
                    Name.bindBidirectional(Value);
                    Count.bind(Total);
                }
            }
            """.trimIndent(),
        )
        assertEquals("one mismatch: $descriptions", 1, descriptions.count { it.contains("E0974") })
        assertTrue(descriptions.any { it.contains("'double' cannot bind to 'String'") })
    }

    fun testBindTypeMismatchSilentWhenUnresolvable() {
        val descriptions = highlightDescriptions(
            """
            package demo;
            public class A {
                public String Name { get; set; } = "";
                public void wire(Widget w) {
                    Name.bind(w.Mystery);
                }
            }
            """.trimIndent(),
        )
        assertFalse("unresolvable side stays silent: $descriptions", descriptions.any { it.contains("E0974") })
    }
}
