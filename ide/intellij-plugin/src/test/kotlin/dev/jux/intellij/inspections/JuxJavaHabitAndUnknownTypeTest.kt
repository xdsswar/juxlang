package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The editor side of the bug-hunt wave: Java utility-class calls with their
 * Jux rewrites, unknown type names in every position a body can write one,
 * anonymous classes over generic interfaces, and catch bodies that jump out
 * past a `finally`.
 */
class JuxJavaHabitAndUnknownTypeTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxJavaHabitInspection(),
            JuxUnresolvedReferenceInspection(),
            JuxUnreachableCodeInspection(),
            JuxMissingReturnInspection(),
        )
    }

    private fun descriptions(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    private fun applyFix(code: String, fix: String): String {
        myFixture.configureByText("a.jux", code.trimIndent())
        myFixture.doHighlighting()
        val action = myFixture.getAllQuickFixes().firstOrNull { it.text == fix }
            ?: error("no fix '$fix' among ${myFixture.getAllQuickFixes().map { it.text }}")
        myFixture.launchAction(action)
        return myFixture.editor.document.text
    }

    fun testUtilityClassesAreReported() {
        val d = descriptions(
            """
            public void main() {
                double d = -2.5;
                print(Math.abs(d));
                print(Integer.parseInt("42"));
                print(String.valueOf(5));
                print(String.join(",", "a"));
                print(Objects.equals(d, d));
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.contains("Jux has no `Math` class") })
        assertTrue(d.toString(), d.any { it.contains("Jux has no boxed number classes") })
        assertTrue(d.toString(), d.any { it.contains("No static method `valueOf` on `String`") })
        assertTrue(d.toString(), d.any { it.contains("No static method `join` on `String`") })
        assertTrue(d.toString(), d.any { it.contains("Jux has no `Objects` class") })
    }

    fun testUserTypeNamedMathIsNotJavas() {
        val d = descriptions(
            """
            class Math {
                public static int twice(int x) { return x * 2; }
            }

            public void main() {
                print(Math.twice(2));
            }
            """,
        )
        assertFalse(d.toString(), d.any { it.contains("Jux has no `Math` class") })
    }

    fun testMathFixUsesTheNumberMethod() {
        assertTrue(applyFix("void f(double x) { print(Math.abs(x)); }", "Replace with '.abs()'").contains("print(x.abs())"))
        assertTrue(applyFix("void f(double x) { print(Math.pow(x, 2.0)); }", "Replace with '.powf()'").contains("print(x.powf(2.0))"))
        assertTrue(applyFix("void f(double a, double b) { print(Math.max(a + 1.0, b)); }", "Replace with '.max()'")
            .contains("print((a + 1.0).max(b))"))
    }

    fun testParseFixAndObjectsFixes() {
        assertTrue(applyFix("void f(String s) { int n = Integer.parseInt(s); }", "Replace with '.parse<int>()'")
            .contains("int n = s.parse<int>();"))
        assertTrue(applyFix("void f(int a, int b) { print(Objects.equals(a, b)); }", "Replace with '=='")
            .contains("print(a == b)"))
        assertTrue(applyFix("void f(String x) { print(String.valueOf(x)); }", "Replace with interpolation")
            .contains("print(\$\"\${x}\")"))
    }

    fun testUnknownTypesInEveryPosition() {
        val d = descriptions(
            """
            class Animal {}

            <T> int count(T x) { return 1; }

            public void main() {
                Animal a = new Animal();
                var v = new Vec<Snark>();
                var c = a as Crab;
                if (a => Dolphin) { print("d"); }
                var k = count<Kraken>(a);
                var arr = new Gleep[3];
                try {
                    print(1);
                } catch (Oyster e) {
                    print(2);
                }
                Zork z = null;
            }
            """,
        )
        for (name in listOf("Snark", "Crab", "Dolphin", "Kraken", "Gleep", "Oyster", "Zork")) {
            assertTrue("$name in $d", d.any { it == "Cannot resolve type '$name'" })
        }
    }

    fun testAnonymousClassOverAGenericInterface() {
        val d = descriptions(
            """
            class Order {
                public int id = 0;
            }

            interface Listener<T> {
                void onEvent(T value);
            }

            public void main() {
                Listener<Order> l = new Listener<Order>() {
                    public void onEvent(Order value) {
                        print(value.id);
                    }
                };
                l.onEvent(new Order());
            }
            """,
        )
        assertFalse(d.toString(), d.any { it.startsWith("Cannot resolve") })
    }

    fun testCatchBodyJumpsPastFinallyAreNotFlagged() {
        val d = descriptions(
            """
            int find(int[] xs) {
                for (var x : xs) {
                    try {
                        if (x < 0) {
                            throw new IllegalStateException("neg");
                        }
                    } catch (IllegalStateException e) {
                        continue;
                    } finally {
                        print("checked");
                    }
                    if (x > 10) {
                        return x;
                    }
                }
                return -1;
            }
            """,
        )
        assertFalse(d.toString(), "Unreachable code" in d)
        assertFalse(d.toString(), "Missing return statement" in d)
    }
}
