package dev.jux.intellij.parser

import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/**
 * Syntax the compiler is landing alongside this plugin, written from the
 * spec: generators (JUX-MISSING-DEFS §M.2), range operators and bodiless
 * interface operators (JUX-OPERATORS §O), member operators overloaded by
 * operand type, the `@export { }` block (JUX-LANG-V1 §8.4), or-patterns
 * binding the same names, and nested destructuring. Each parses without an
 * error, and its names resolve and rename.
 */
class JuxLandingSyntaxTest : BasePlatformTestCase() {

    private fun parses(name: String, text: String) {
        myFixture.configureByText("$name.jux", text)
        val errors = PsiTreeUtil.collectElementsOfType(myFixture.file, PsiErrorElement::class.java)
        assertTrue("$name: " + errors.joinToString { "${it.errorDescription} @${it.textOffset}" }, errors.isEmpty())
    }

    private fun count(type: com.intellij.psi.tree.IElementType): Int =
        PsiTreeUtil.collectElements(myFixture.file) { it.elementType === type }.size

    /** Resolve the reference at `<caret>`; the target's name and element type. */
    private fun target(text: String): Pair<String?, Any?> {
        myFixture.configureByText("a.jux", text)
        val resolved = myFixture.file.findReferenceAt(myFixture.caretOffset)?.resolve()
        return (resolved as? JuxNamedElement)?.name to resolved?.elementType
    }

    // ---- generators ------------------------------------------------------------

    fun testGeneratorsParse() {
        parses(
            "gen",
            """
            Iterator<int> naturals(int n) {
                for (int i = 0; i < n; i++) { yield i; }
            }
            Iterator<int> chained(Iterator<int> a, Iterator<int> b) {
                yield* a;
                yield* b;
            }
            async Stream<String> lines() {
                yield "a";
                if (true) yield "b";
            }
            async void consume() {
                for await (var line : lines()) { print(line); }
            }
            """.trimIndent(),
        )
    }

    fun testYieldedLocalResolves() {
        val (name, type) = target("Iterator<int> f() { int v = 1; yield <caret>v; }")
        assertEquals("v", name)
        assertEquals(E.LOCAL_VARIABLE, type)
    }

    fun testYieldStarOperandResolves() {
        val (name, type) = target("Iterator<int> f(Iterator<int> src) { yield* sr<caret>c; }")
        assertEquals("src", name)
        assertEquals(E.PARAMETER, type)
    }

    fun testForAwaitVariableResolves() {
        val (name, _) = target("async Stream<String> lines() { yield \"a\"; }\nasync void c() { for await (var line : lines()) { print(li<caret>ne); } }")
        assertEquals("line", name)
    }

    // ---- operators ---------------------------------------------------------------

    fun testRangeAndInterfaceOperatorsParse() {
        parses(
            "ops",
            """
            record Day(int n) {
                public Range<Day> operator..(Day end) { return new Range<Day>(this, end); }
                public Range<Day> operator..=(Day end) { return new Range<Day>(this, end); }
            }
            interface Addable<T> {
                T operator+(T other);
                T operator-(T other);
            }
            record V(int x) implements Addable<V> {
                public V operator+(V other) { return new V(x + other.x); }
                public V operator-(V other) { return new V(x - other.x); }
            }
            <T extends Addable<T>> T sum(T a, T b) { return a + b; }
            """.trimIndent(),
        )
        assertEquals(6, count(E.OPERATOR_DECLARATION))
    }

    fun testOverloadedMemberOperatorsAreDistinct() {
        parses(
            "overloads",
            """
            record V(int x) {
                public V operator*(int k) { return new V(x * k); }
                public V operator*(V other) { return new V(x * other.x); }
            }
            """.trimIndent(),
        )
        assertEquals(2, count(E.OPERATOR_DECLARATION))
    }

    fun testOperatorParameterResolves() {
        val (name, type) = target("record V(int x) { public V operator*(int k) { return new V(x * <caret>k); } }")
        assertEquals("k", name)
        assertEquals(E.PARAMETER, type)
    }

    // ---- @export block -----------------------------------------------------------

    fun testExportBlockParsesAndItsFunctionsResolve() {
        parses(
            "export",
            """
            @export {
                public int add(int a, int b) { return a + b; }
                public int twice(int a) { return add(a, a); }
            }
            @export(name = "v2")
            public int three() { return 3; }
            """.trimIndent(),
        )
        val (name, type) = target(
            "@export {\n    public int add(int a, int b) { return a + b; }\n    public int twice(int a) { return ad<caret>d(a, a); }\n}",
        )
        assertEquals("add", name)
        assertEquals(E.METHOD_DECLARATION, type)
    }

    fun testExportBlockReformatsAndShowsInStructure() {
        myFixture.configureByText("Ffi.jux", "@export {\npublic int add(int a, int b) {\nreturn a + b;\n}\n}\nvoid main() {}")
        com.intellij.openapi.command.WriteCommandAction.runWriteCommandAction(project) {
            com.intellij.psi.codeStyle.CodeStyleManager.getInstance(project).reformat(myFixture.file)
        }
        assertEquals(
            "@export {\n    public int add(int a, int b) {\n        return a + b;\n    }\n}\nvoid main() {}",
            myFixture.file.text,
        )
        myFixture.testStructureView { component ->
            com.intellij.testFramework.PlatformTestUtil.expandAll(component.tree)
            com.intellij.testFramework.PlatformTestUtil.assertTreeEqual(component.tree, "-Ffi.jux\n add\n main")
        }
    }

    // ---- patterns ----------------------------------------------------------------

    fun testOrPatternBindsTheSameName() {
        val text = """
            sealed interface Expr {}
            record Num(int v) implements Expr {}
            record Neg(int v) implements Expr {}
            int value(Expr e) { return switch (e) { case Num(var n) | Neg(var n) -> <caret>n; default -> 0; }; }
        """.trimIndent()
        parses("orPattern", text.replace("<caret>", ""))
        val (name, type) = target(text)
        assertEquals("n", name)
        assertEquals(E.LOCAL_VARIABLE, type)
    }

    fun testUnusedPatternBinderIsFlaggedWithoutARemoveFix() {
        myFixture.enableInspections(dev.jux.intellij.inspections.JuxUnusedLocalSymbolInspection())
        myFixture.configureByText(
            "a.jux",
            "record C(double r) {}\ndouble f(Object o) { return switch (o) { case C(var r<caret>ad) -> 1.0; default -> 0.0; }; }",
        )
        val unused = myFixture.doHighlighting().filter { it.description == "Variable 'rad' is never used" }
        assertEquals(1, unused.size)
        assertTrue(
            "no quick-fix may delete a binder out of its pattern",
            myFixture.getAllQuickFixes().none { it.text.contains("Remove", ignoreCase = true) },
        )
    }

    fun testNestedDestructuring() {
        val text = """
            record Pt(int x, int y) {}
            record Line(Pt start, Pt end) {}
            void m(Line l) { var Line(Pt(a, b), var end) = l; print(a + b + <caret>end.x); }
        """.trimIndent()
        parses("nested", text.replace("<caret>", ""))
        assertEquals(3, count(E.LOCAL_VARIABLE))
        val (name, _) = target(text)
        assertEquals("end", name)
    }

    fun testRenameNestedDestructuredBinder() {
        myFixture.configureByText(
            "a.jux",
            "record Pt(int x, int y) {}\nrecord Line(Pt start, Pt end) {}\nvoid m(Line l) { var Line(Pt(a<caret>, b), var end) = l; print(a); }",
        )
        myFixture.renameElement(myFixture.elementAtCaret, "left")
        myFixture.checkResult(
            "record Pt(int x, int y) {}\nrecord Line(Pt start, Pt end) {}\nvoid m(Line l) { var Line(Pt(left, b), var end) = l; print(left); }",
        )
    }
}
