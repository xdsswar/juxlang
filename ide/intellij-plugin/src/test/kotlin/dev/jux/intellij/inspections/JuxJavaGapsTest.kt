package dev.jux.intellij.inspections

import com.intellij.codeInsight.completion.CompletionType
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Pass-2 Java-parity features: implement/override completion in a class
 * body, "Create missing 'case' branches", "'if' statement can be simplified"
 * and "Indexed loop can be a for-each": what each offers, what it leaves
 * alone, and what it writes.
 */
class JuxJavaGapsTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(JuxSimplifiableIfInspection(), JuxIndexedLoopInspection())
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

    private fun intention(code: String, text: String): String {
        myFixture.configureByText("a.jux", code.trimIndent())
        val action = myFixture.availableIntentions.firstOrNull { it.text == text }
            ?: error("no intention '$text' among ${myFixture.availableIntentions.map { it.text }}")
        myFixture.launchAction(action)
        return myFixture.editor.document.text
    }

    // ---- 'if' can be simplified --------------------------------------------------

    fun testSimplifiableIfShapes() {
        val d = descriptions(
            """
            bool a(int n) { if (n > 0) return true; else return false; }
            bool b(bool done) { if (done) { return false; } return true; }
            void c(bool ok) { bool flag = false; if (ok) flag = true; else flag = false; print(flag); }
            bool d(int n) { if (n > 0) { print(n); return true; } return false; }
            bool e(int n) { if (n > 0) return true; else return true; }
            """,
        )
        assertEquals(d.toString(), 3, d.count { it == "'if' statement can be simplified" })
    }

    fun testSimplifiableIfFixes() {
        assertEquals("bool a(int n) { return n > 0; }",
            applyFix("bool a(int n) { if (n > 0) return true; else return false; }", "Replace 'if' with 'return n > 0;'"))
        assertEquals("bool b(bool done) { return !done; }",
            applyFix("bool b(bool done) { if (done) { return false; } return true; }", "Replace 'if' with 'return !done;'"))
    }

    // ---- indexed loop -------------------------------------------------------------

    fun testIndexedLoopShapes() {
        val d = descriptions(
            """
            void a(Vec<int> xs) { for (int i = 0; i < xs.len(); i++) { print(xs[i]); } }
            void b(int[] xs) { for (var i : 0..xs.length) { print(xs[i] * 2); } }
            void c(Vec<int> xs) { for (int i = 0; i < xs.len(); i++) { print(i + xs[i]); } }
            void d(Vec<int> xs) { for (int i = 0; i < xs.len(); i++) { xs[i] = 0; } }
            void e(Vec<int> xs) { for (int i = 1; i < xs.len(); i++) { print(xs[i]); } }
            void f(Vec<int> xs, Vec<int> ys) { for (int i = 0; i < xs.len(); i++) { print(ys[i]); } }
            """,
        )
        assertEquals(d.toString(), 2, d.count { it.startsWith("Indexed loop can be a for-each") })
    }

    fun testIndexedLoopFix() {
        val fixed = applyFix(
            "void a(Vec<int> items) { for (int i = 0; i < items.len(); i++) { print(items[i]); } }",
            "Replace with for-each",
        )
        assertEquals("void a(Vec<int> items) { for (var item : items) { print(item); } }", fixed)
    }

    // ---- missing case branches -------------------------------------------------------

    fun testMissingEnumCases() {
        val out = intention(
            """
            enum Light { Red, Amber, Green }
            void show(Light l) {
                sw<caret>itch (l) {
                    case Red -> print("stop");
                }
            }
            """,
            "Create 2 missing 'case' branches",
        )
        assertTrue(out, out.contains("case Amber -> {"))
        assertTrue(out, out.contains("case Green -> {"))
        assertFalse(out, out.contains("case Red -> {"))
    }

    fun testMissingSealedCasesInAnExpression() {
        val out = intention(
            """
            sealed interface Shape permits Circle, Square {}
            record Circle(double r) implements Shape {}
            record Square(double side) implements Shape {}
            double area(Shape s) {
                return sw<caret>itch (s) {
                    case Circle c -> 3.14 * c.r * c.r;
                };
            }
            """,
            "Create missing 'case' branch",
        )
        assertTrue(out, out.contains("case Square square -> throw new UnsupportedOperationException(\"TODO: Square square\");"))
    }

    fun testNoMissingCasesWithDefault() {
        myFixture.configureByText(
            "a.jux",
            """
            enum Light { Red, Amber, Green }
            void show(Light l) { sw<caret>itch (l) { case Red -> print(1); default -> print(2); } }
            """.trimIndent(),
        )
        assertTrue(myFixture.availableIntentions.none { it.text.contains("missing 'case'") })
    }

    // ---- implement / override completion ------------------------------------------

    fun testImplementCompletionWritesTheMember() {
        myFixture.configureByText(
            "a.jux",
            """
            interface Shape {
                double area();
            }
            class Circle implements Shape {
                double r;
                ar<caret>
            }
            """.trimIndent(),
        )
        val items = myFixture.complete(CompletionType.BASIC)
        if (items != null) {
            val item = items.firstOrNull { it.lookupString == "area" } ?: error("no 'area' among ${items.map { it.lookupString }}")
            myFixture.lookup.currentItem = item
            myFixture.finishLookup('\n')
        }
        val text = myFixture.editor.document.text
        assertTrue(text, text.contains("@override\n    public double area() {"))
    }

    fun testImplementCompletionReplacesTypedModifiers() {
        myFixture.configureByText(
            "a.jux",
            """
            abstract class Base {
                public abstract int size();
            }
            class Box extends Base {
                public si<caret>
            }
            """.trimIndent(),
        )
        val items = myFixture.complete(CompletionType.BASIC)
        if (items != null) {
            val item = items.firstOrNull { it.lookupString == "size" } ?: error("no 'size' among ${items.map { it.lookupString }}")
            myFixture.lookup.currentItem = item
            myFixture.finishLookup('\n')
        }
        val text = myFixture.editor.document.text
        assertFalse(text, text.contains("public @override"))
        assertTrue(text, text.contains("    @override\n    public int size() {"))
    }
}
