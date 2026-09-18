package dev.jux.intellij.inspections

import com.intellij.codeInsight.daemon.impl.HighlightInfo
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Java's everyday inspections, ported: what each reports, what it leaves
 * alone (the cases a careless port gets wrong), and what its fix writes.
 */
class JuxJavaParityInspectionsTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxEmptyCatchBlockInspection(),
            JuxRedundantCastInspection(),
            JuxConstantConditionInspection(),
            JuxRedundantElseInspection(),
            JuxPointlessBooleanExpressionInspection(),
            JuxUnusedPrivateMemberInspection(),
            JuxDuplicateConditionInspection(),
            JuxSelfAssignmentInspection(),
            JuxEmptyStatementBodyInspection(),
            JuxMissingReturnInspection(),
            JuxAbstractMethodInClassInspection(),
            JuxUnhandledExceptionInspection(),
            JuxUnusedLocalSymbolInspection(),
        )
    }

    private fun highlights(code: String): List<HighlightInfo> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting()
    }

    private fun descriptions(code: String): List<String> = highlights(code).mapNotNull { it.description }

    /** Applies the quick fix named [fix] (anywhere in the file) and returns the text. */
    private fun applyFix(code: String, fix: String): String {
        myFixture.configureByText("a.jux", code.trimIndent())
        myFixture.doHighlighting()
        val action = myFixture.getAllQuickFixes().firstOrNull { it.text == fix }
            ?: error("no fix '$fix' among ${myFixture.getAllQuickFixes().map { it.text }}")
        myFixture.launchAction(action)
        return myFixture.editor.document.text
    }

    // ---- empty catch ------------------------------------------------------

    fun testEmptyCatchReportedButCommentAndIgnoredAreNot() {
        val d = descriptions(
            """
            void f() {
                try { g(); } catch (Exception e) { }
                try { g(); } catch (Exception e) { // expected
                }
                try { g(); } catch (Exception ignored) { }
            }
            void g() {}
            """,
        )
        assertEquals(d.toString(), 1, d.count { it == "Empty 'catch' block" })
    }

    fun testEmptyCatchFixes() {
        val renamed = applyFix("void f() {\n    try { g(); } catch (Exception e) { }\n}\nvoid g() {}", "Rename 'catch' parameter to 'ignored'")
        assertTrue(renamed, renamed.contains("catch (Exception ignored)"))
        val unwrapped = applyFix("void f() {\n    try {\n        g();\n    } catch (Exception e) {\n    }\n}\nvoid g() {}", "Delete 'catch' clause")
        assertFalse(unwrapped, unwrapped.contains("try"))
        assertTrue(unwrapped, unwrapped.contains("g();"))
    }

    // ---- redundant cast ---------------------------------------------------

    fun testRedundantCastOnlyWhenTypesAreExactlyEqual() {
        val d = descriptions(
            """
            void f(int a, long b) {
                int x = (int) a;
                long y = (long) a;
                int z = (int) b;
                int w = (int) 5L;
                double v = (double) 1.5;
            }
            """,
        )
        assertEquals(d.toString(), 2, d.count { it.endsWith("is redundant") })
        assertTrue(d.toString(), d.contains("Casting 'a' to 'int' is redundant"))
        assertTrue(d.toString(), d.contains("Casting '1.5' to 'double' is redundant"))
    }

    fun testRemoveRedundantCast() {
        val text = applyFix("void f(int a) {\n    int x = (int) a;\n}", "Remove redundant cast")
        assertTrue(text, text.contains("int x = a;"))
    }

    // ---- constant condition -----------------------------------------------

    fun testConstantConditions() {
        val d = descriptions(
            """
            void f(int n, double d) {
                if (true) { g(); }
                while (false) { g(); }
                while (true) { break; }
                bool a = n == n;
                bool b = d != d;
                bool c = 1 < 2;
            }
            void g() {}
            """,
        )
        assertEquals(d.toString(), 1, d.count { it == "Condition is always true" })
        assertEquals(d.toString(), 1, d.count { it == "Condition is always false" })
        assertTrue(d.toString(), d.contains("Condition 'n == n' is always true"))
        assertTrue(d.toString(), d.contains("Condition '1 < 2' is always true"))
        assertFalse("NaN test: $d", d.any { it.contains("d != d") })
    }

    fun testSimplifyConstantIf() {
        val text = applyFix(
            "void f() {\n    if (false) {\n        g();\n    } else {\n        h();\n    }\n}\nvoid g() {}\nvoid h() {}",
            "Simplify 'if' with constant condition",
        )
        assertFalse(text, text.contains("if"))
        assertTrue(text, text.contains("    h();"))
        assertFalse(text, text.contains("    g();"))
    }

    // ---- pointless boolean ------------------------------------------------

    fun testPointlessBooleanSimplifications() {
        assertTrue(applyFix("bool f(bool b) {\n    return b == true;\n}", "Simplify to 'b'").contains("return b;"))
        assertTrue(applyFix("bool f(bool b) {\n    return b == false;\n}", "Simplify to '!b'").contains("return !b;"))
        assertTrue(applyFix("bool f() {\n    return !true;\n}", "Simplify to 'false'").contains("return false;"))
        // A call may do something: `f() && false` must still call f().
        val d = descriptions("bool g() { return true; }\nbool f() {\n    return g() && false;\n}")
        assertFalse(d.toString(), d.any { it.contains("can be simplified") })
    }

    // ---- redundant else ---------------------------------------------------

    fun testRedundantElseRemoved() {
        myFixture.configureByText(
            "a.jux",
            "int f(bool c) {\n    if (c) {\n        return 1;\n    } el<caret>se {\n        g();\n    }\n    return 2;\n}\nvoid g() {}",
        )
        myFixture.launchAction(myFixture.findSingleIntention("Remove redundant 'else'"))
        val text = myFixture.editor.document.text
        assertFalse(text, text.contains("else"))
        assertTrue(text, text.contains("    }\n    g();\n    return 2;"))
    }

    fun testElseKeptWhenAnOuterBranchFallsThrough() {
        // Moving `x()` out would run it after `y()` too.
        val infos = highlights(
            """
            void f(bool a, bool b) {
                if (a) { y(); } else if (b) { return; } else { x(); }
            }
            void x() {}
            void y() {}
            """,
        )
        assertFalse(infos.any { it.description == "Redundant 'else'" })
    }

    // ---- unused private members -------------------------------------------

    fun testUnusedPrivateMembers() {
        val d = descriptions(
            """
            public class A {
                private static int counter = 0;
                private static int used = 1;
                public A() {}
                private A(int seed) {}
                private void helper() {}
                private void called() {}
                public int run() { called(); return used; }
            }
            enum Color {
                RED(1);
                private Color(int v) {}
            }
            """,
        )
        assertTrue(d.toString(), d.contains("Private field 'counter' is never used"))
        assertTrue(d.toString(), d.contains("Private method 'helper' is never used"))
        assertTrue(d.toString(), d.contains("Private constructor 'A(...)' is never used"))
        assertFalse(d.toString(), d.any { it.contains("'used'") || it.contains("'called'") || it.contains("Color") })
    }

    fun testSafeDeleteUnusedPrivateMethod() {
        val text = applyFix(
            "public class A {\n    /** Help. */\n    private void helper() {}\n\n    public void run() {}\n}",
            "Safe delete unused method",
        )
        assertFalse(text, text.contains("helper"))
        assertFalse(text, text.contains("Help."))
        assertTrue(text, text.contains("public void run() {}"))
    }

    fun testRemoveUnusedParameterUpdatesCallers() {
        val text = applyFix(
            """
            public class A {
                private int add(int a, int b) { return a; }
                public int run() { return add(1, 2) + add(3, 4); }
            }
            """,
            "Remove unused parameter",
        )
        assertTrue(text, text.contains("private int add(int a)"))
        assertTrue(text, text.contains("add(1) + add(3)"))
    }

    // ---- duplicate condition / self-assignment / empty body -----------------

    fun testDuplicateConditionRemoved() {
        val text = applyFix(
            "void f(bool a, bool b) {\n    if (a) {\n        x();\n    } else if (b) {\n        y();\n    } else if (a) {\n        z();\n    }\n}\nvoid x() {}\nvoid y() {}\nvoid z() {}",
            "Remove branch with duplicate condition",
        )
        assertFalse(text, text.contains("z();\n    }"))
        assertTrue(text, text.contains("else if (b)"))
    }

    fun testSelfAssignment() {
        val d = descriptions(
            """
            public class A {
                private int x;
                public int Count { get; set; } = 0;
                public A(int x) { x = x; this.x = x; Count = Count; }
            }
            """,
        )
        assertEquals(d.toString(), 1, d.count { it == "Variable 'x' is assigned to itself" })
        assertFalse(d.toString(), d.any { it.contains("'Count'") })
    }

    fun testEmptyStatementBody() {
        val d = descriptions(
            """
            bool poll() { return false; }
            void f(bool c) {
                if (c);
                if (c) {}
                if (c) {} else { poll(); }
                while (poll()) {}
                while (c);
                if (c) { /* later */ }
            }
            """,
        )
        assertEquals(d.toString(), 2, d.count { it == "'if' statement has empty body" })
        assertEquals(d.toString(), 1, d.count { it == "'while' statement has empty body" })
    }

    // ---- missing return ---------------------------------------------------

    fun testMissingReturn() {
        val d = descriptions(
            """
            int a(bool c) { if (c) { return 1; } }
            int b(bool c) { if (c) { return 1; } else { return 2; } }
            int d() { while (true) { } }
            int e(int n) { switch (n) { case 1 -> { return 1; } default -> { return 0; } } }
            int f(int n) { try { return n; } catch (Exception ex) { throw ex; } }
            int g(bool c) { do { return 1; } while (c); }
            int h() { while (true) { if (h() > 0) { break; } } }
            void i() { }
            """,
        )
        // Only `a` and `h` can fall off the end.
        assertEquals(d.toString(), 2, d.count { it == "Missing return statement" })
    }

    fun testAddReturnAndMakeVoid() {
        val added = applyFix("int a(bool c) {\n    if (c) {\n        return 1;\n    }\n}", "Add 'return' statement")
        assertTrue(added, added.contains("    return 0;\n}"))
        val voided = applyFix("int a() {\n    b();\n}\nvoid b() {}", "Make 'a' return 'void'")
        assertTrue(voided, voided.startsWith("void a()"))
    }

    // ---- abstract method in class -----------------------------------------

    fun testAbstractMethodInConcreteClass() {
        val text = applyFix("public class Shape {\n    public abstract double area();\n}", "Make 'Shape' abstract")
        assertTrue(text, text.startsWith("public abstract class Shape"))
        val bodied = applyFix("public class Shape {\n    public abstract double area();\n}", "Make 'area' not abstract")
        assertTrue(bodied, bodied.contains("public double area() {"))
        assertTrue(bodied, bodied.contains("throw new UnsupportedOperationException();"))
    }

    // ---- unhandled checked exceptions --------------------------------------

    fun testUnhandledCheckedExceptions() {
        val d = descriptions(
            """
            class ConfigError extends Exception {}
            class Bug extends RuntimeException {}
            class Deep extends ConfigError {}
            class Loader {
                void a() { throw new ConfigError(); }
                void b() throws ConfigError { throw new Deep(); }
                void c() { try { throw new Deep(); } catch (ConfigError e) { } }
                void d() { throw new Bug(); }
                void e() { b(); }
                void f() { Runnable r = () -> { throw new ConfigError(); }; }
            }
            """,
        )
        // `a()` throws it undeclared; `e()` calls `b()`, which declares it.
        // `Deep` is caught or declared, `Bug` is unchecked, the lambda is skipped.
        assertEquals(d.toString(), 2, d.count { it == "Unhandled exception: ConfigError" })
        assertFalse(d.toString(), d.any { it.contains("Bug") || it.contains("Deep") })
    }

    fun testAddExceptionToThrows() {
        val text = applyFix(
            "class ConfigError extends Exception {}\nclass Loader {\n    void a() {\n        throw new ConfigError();\n    }\n}",
            "Add exception to method signature",
        )
        assertTrue(text, text.contains("void a() throws ConfigError {"))
    }
}
