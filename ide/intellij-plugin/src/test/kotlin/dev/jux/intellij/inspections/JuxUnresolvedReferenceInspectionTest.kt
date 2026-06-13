package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The unresolved-reference inspection: it must flag a name orphaned by a rename
 * (or a typo) while never red-underlining a name that is legitimately bound —
 * loop / catch vars, parameters, imports, built-ins, cross-file symbols.
 */
class JuxUnresolvedReferenceInspectionTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(JuxUnresolvedReferenceInspection())
    }

    private fun descriptions(code: String, fileName: String = "a.jux"): List<String> {
        myFixture.configureByText(fileName, code)
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    private fun assertUnresolved(code: String, name: String) {
        val d = descriptions(code)
        assertTrue(
            "expected an unresolved diagnostic for '$name' in $d",
            d.any { it == "Cannot resolve symbol '$name'" || it == "Cannot resolve type '$name'" },
        )
    }

    private fun assertAllResolved(code: String) {
        val d = descriptions(code)
        assertFalse("unexpected unresolved diagnostic in $d", d.any { it.startsWith("Cannot resolve symbol") })
    }

    // ---- positive: genuinely unknown / orphaned-by-rename -------------------

    fun testOrphanedUsageFlagged() {
        // `total` is used but nothing declares it (the declaration was renamed).
        assertUnresolved(
            """
            package demo;
            public class A {
                public int go() {
                    var count = 1;
                    return total;
                }
            }
            """.trimIndent(),
            "total",
        )
    }

    fun testTypoCallFlagged() {
        assertUnresolved(
            """
            package demo;
            public class A {
                public void helper() {}
                public void go() { helprr(); }
            }
            """.trimIndent(),
            "helprr",
        )
    }

    // ---- negatives: every legitimate binding form must stay quiet -----------

    fun testValidLocalNotFlagged() {
        assertAllResolved(
            """
            package demo;
            public class A {
                public int go() {
                    var count = 1;
                    return count;
                }
            }
            """.trimIndent(),
        )
    }

    fun testForEachVariableNotFlagged() {
        // `item` is a for-each binding (a raw identifier, not a LOCAL_VARIABLE
        // node) — the file-wide binding census must still cover it.
        assertAllResolved(
            """
            package demo;
            public class A {
                public void go(List<int> items) {
                    for (var item : items) {
                        print(item);
                    }
                }
            }
            """.trimIndent(),
        )
    }

    fun testCatchVariableNotFlagged() {
        assertAllResolved(
            """
            package demo;
            public class A {
                public void risky() {}
                public void go() {
                    try { risky(); } catch (Error e) { print(e); }
                }
            }
            """.trimIndent(),
        )
    }

    fun testBuiltinAndCapitalizedNotFlagged() {
        // `print` is a built-in global; `Singleton` is capitalized (a type / std
        // symbol the LSP owns) — neither is the IDE-side resolver's to reject.
        assertAllResolved(
            """
            package demo;
            public class A {
                public Object go() {
                    print("hi");
                    return Singleton;
                }
            }
            """.trimIndent(),
        )
    }

    fun testMemberAccessNotFlagged() {
        // `unknownMember` is a member access — left to the language server.
        assertAllResolved(
            """
            package demo;
            public class A {
                public void go(Widget w) { w.unknownMember(); }
            }
            """.trimIndent(),
        )
    }

    fun testWildcardImportSuppressesEntirely() {
        assertAllResolved(
            """
            package demo;
            import rust.std.collections.*;
            public class A {
                public void go() { mysteryFromWildcard(); }
            }
            """.trimIndent(),
        )
    }

    fun testCrossFileSymbolNotFlagged() {
        // A symbol declared in another project file is never "unknown".
        myFixture.addFileToProject(
            "model.jux",
            "package demo;\npublic class Model { public void shared() {} }\n",
        )
        assertAllResolved(
            """
            package demo;
            public class A {
                public void go() {
                    var m = new Model();
                    m.shared();
                    var s = shared;
                }
            }
            """.trimIndent(),
        )
    }

    // ---- type positions -----------------------------------------------------

    fun testUnknownTypeFlagged() {
        // `Wodget` names no in-file/project/prelude type and is not imported.
        assertUnresolved(
            """
            package demo;
            public class A {
                public void go(Wodget w) {}
            }
            """.trimIndent(),
            "Wodget",
        )
    }

    fun testRenamedAwayTypeUsageFlagged() {
        // `Gadget` is defined, but `Widget` (a stale reference after a rename)
        // resolves to nothing.
        assertUnresolved(
            """
            package demo;
            public class Gadget {}
            public class A {
                public Widget make() { return null; }
            }
            """.trimIndent(),
            "Widget",
        )
    }

    fun testInFileTypeNotFlagged() {
        assertAllResolved(
            """
            package demo;
            public class Gadget {}
            public class A {
                public Gadget make() { return new Gadget(); }
            }
            """.trimIndent(),
        )
    }

    fun testPreludeAndPrimitiveTypesNotFlagged() {
        // Map / List / Throwable are jux.std prelude; int / String are built-in.
        assertAllResolved(
            """
            package demo;
            public class A {
                public Map<String, List<int>> data;
                public void boom() throws Throwable {}
            }
            """.trimIndent(),
        )
    }

    fun testTypeParameterNotFlagged() {
        assertAllResolved(
            """
            package demo;
            public class Box<T> {
                private T value;
                public T get() { return value; }
                public <R> R map(R seed) { return seed; }
            }
            """.trimIndent(),
        )
    }

    fun testImportedTypeNotFlagged() {
        assertAllResolved(
            """
            package demo;
            import rust.std.collections.BTreeMap;
            public class A {
                public BTreeMap<int, int> m;
            }
            """.trimIndent(),
        )
    }

    fun testCrossFileTypeNotFlagged() {
        myFixture.addFileToProject("beast.jux", "package demo;\npublic class Beast {}\n")
        assertAllResolved(
            """
            package demo;
            public class A {
                public void pet(Beast b) {}
            }
            """.trimIndent(),
        )
    }

    fun testQualifiedTypeNotFlagged() {
        // A dotted type path is package resolution — left to the language server.
        assertAllResolved(
            """
            package demo;
            public class A {
                public foo.bar.Whatever thing() { return null; }
            }
            """.trimIndent(),
        )
    }

    fun testChangeToNearestTypeQuickFix() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class Gadget {}
            public class A {
                public Gadg<caret>ey make() { return null; }
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val fix = myFixture.findSingleIntention("Change to 'Gadget'")
        myFixture.launchAction(fix)
        assertTrue("type usage rewritten to the in-project type", myFixture.file.text.contains("public Gadget make()"))
    }

    // ---- quick-fix ----------------------------------------------------------

    fun testChangeToNearestQuickFix() {
        myFixture.configureByText(
            "a.jux",
            """
            package demo;
            public class A {
                public int go() {
                    var count = 1;
                    return co<caret>nt;
                }
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val fix = myFixture.findSingleIntention("Change to 'count'")
        myFixture.launchAction(fix)
        assertTrue("usage rewritten to the nearest name", myFixture.file.text.contains("return count;"))
    }
}
