package dev.jux.intellij.resolve

import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.inspections.JuxGeneratorInspection
import dev.jux.intellij.inspections.JuxInterfaceOperatorInspection
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * The editor's side of three newer language features:
 *
 * - operators overloaded by operand type (§O.2.3), `..` / `..=` operators
 *   (§O.2.4) and operators in interfaces (§7.14.6): the use resolves to the
 *   member the compiler calls, has that member's return type, and Find
 *   Usages goes the other way;
 * - interface operators show implement markers;
 * - the generator (E0990/E0994/E0995/E0996/E0997) and interface-operator
 *   (E0936) inspections, with their fixes.
 */
class JuxOperatorsAndGeneratorsTest : BasePlatformTestCase() {

    private val vec2 = """
        class Vec2 {
            public double x;
            public double y;
            public Vec2(double x, double y) { this.x = x; this.y = y; }
            public Vec2 operator*(double k) { return new Vec2(x * k, y * k); }
            public double operator*(Vec2 other) { return x * other.x + y * other.y; }
            public Vec2 operator+(Vec2 other) { return new Vec2(x + other.x, y + other.y); }
        }
        class Point extends Vec2 {
            public Point(double x, double y) { super(x, y); }
        }
        record Money(long cents) {
            public Money operator+(Money other) { return new Money(cents + other.cents); }
            public Money operator+(long more) { return new Money(cents + more); }
        }
    """.trimIndent()

    /** The operator declaration the use at `<caret>` resolves to, as `symbol(paramType)`. */
    private fun targetAt(code: String): String? {
        myFixture.configureByText("a.jux", code)
        val ref = myFixture.file.findReferenceAt(myFixture.caretOffset) ?: return null
        val decl = ref.resolve() ?: return null
        val param = JuxHierarchy.parameters(decl).firstOrNull()
        val type = param?.let { JuxTypeEngine.declaredType(it).presentable() }
        return "${JuxOperators.symbolOf(decl)}($type)"
    }

    fun testOverloadPickedByOperandType() {
        assertEquals("*(double)", targetAt("$vec2\nvoid main() { var a = new Vec2(1.0, 2.0); var d = a <caret>* 2.0; }"))
        assertEquals("*(Vec2)", targetAt("$vec2\nvoid main() { var a = new Vec2(1.0, 2.0); var d = a <caret>* a; }"))
    }

    fun testSubclassUsesTheInheritedSet() {
        assertEquals("*(Vec2)", targetAt("$vec2\nvoid main() { var p = new Point(1.0, 1.0); var d = p <caret>* p; }"))
    }

    fun testCompoundAssignmentPicksLikeTheOperator() {
        assertEquals("+(long)", targetAt("$vec2\nvoid main() { var price = new Money(1); price <caret>+= 1; }"))
        assertEquals("+(Money)", targetAt("$vec2\nvoid main() { var price = new Money(1); price <caret>+= new Money(2); }"))
    }

    fun testPrimitiveOperatorResolvesToNothing() {
        assertNull(targetAt("void main() { var n = 1 <caret>+ 2; }"))
    }

    fun testExpressionTypeComesFromTheChosenOperator() {
        myFixture.configureByText("a.jux", "$vec2\nvoid main() { var a = new Vec2(1.0, 2.0); var s = a * 2.0; var d = a * a; }")
        val types = PsiTreeUtil.collectElements(myFixture.file) { it.elementType === E.BINARY_EXPRESSION }
            .filter { it.text.startsWith("a * ") }
            .map { JuxTypeEngine.typeOf(it).presentable() }
        assertEquals(listOf("Vec2", "double"), types)
    }

    fun testRangeOperatorResolvesAndTypes() {
        val code = """
            class Days {}
            record Date(int day) {
                public Days operator..(Date end) { return new Days(); }
                public Days operator..=(Date end) { return new Days(); }
            }
            void main() {
                var a = new Date(1);
                var b = new Date(3);
                for (var d : a<caret>..=b) { print(d); }
            }
        """.trimIndent()
        assertEquals("..=(Date)", targetAt(code))
        val range = PsiTreeUtil.collectElements(myFixture.file) { it.elementType === E.RANGE_EXPRESSION }.single()
        assertEquals("Days", JuxTypeEngine.typeOf(range).presentable())
    }

    fun testBoundedTypeParameterReachesTheInterfaceOperator() {
        val code = """
            interface Addable<T> { T operator+(T other); }
            <T extends Addable<T>> T sum(T a, T b) { return a <caret>+ b; }
        """.trimIndent()
        assertEquals("+(T)", targetAt(code))
    }

    fun testFindUsagesOfAnOverloadCountsOnlyItsOwnUses() {
        myFixture.configureByText(
            "a.jux",
            "$vec2\nvoid main() { var a = new Vec2(1.0, 2.0); var s = a * 2.0; var t = a * 3.0; var d = a * a; }",
        )
        val byDouble = operator("*", "double")
        val byVec = operator("*", "Vec2")
        assertEquals(2, myFixture.findUsages(byDouble).size)
        assertEquals(1, myFixture.findUsages(byVec).size)
    }

    fun testInterfaceOperatorGutters() {
        myFixture.configureByText(
            "a.jux",
            """
            interface Addable<T> { T operator+(T other); }
            record Money(long cents) implements Addable<Money> {
                public Money operator+(Money other) { return new Money(cents + other.cents); }
            }
            """.trimIndent(),
        )
        val tips = myFixture.findAllGutters().mapNotNull { it.tooltipText }
        assertTrue(tips.toString(), tips.any { it.contains("Implements operator+ in 'Addable'") })
        assertTrue(tips.toString(), tips.any { it.contains("Is implemented in") })
    }

    private fun operator(symbol: String, paramType: String): PsiElement =
        PsiTreeUtil.collectElements(myFixture.file) { it.elementType === E.OPERATOR_DECLARATION }.single {
            JuxOperators.symbolOf(it) == symbol &&
                JuxHierarchy.parameters(it).firstOrNull()?.let { p -> JuxTypeEngine.declaredType(p).presentable() } == paramType
        }

    // ---- generator inspection ------------------------------------------------

    private fun errors(code: String): List<String> {
        myFixture.enableInspections(JuxGeneratorInspection(), JuxInterfaceOperatorInspection())
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting().mapNotNull { it.description }.filter { it.contains("(E0") }
    }

    private fun applyFix(code: String, fix: String): String {
        myFixture.enableInspections(JuxGeneratorInspection(), JuxInterfaceOperatorInspection())
        myFixture.configureByText("a.jux", code.trimIndent())
        myFixture.doHighlighting()
        val action = myFixture.getAllQuickFixes().firstOrNull { it.text == fix }
            ?: error("no fix '$fix' among ${myFixture.getAllQuickFixes().map { it.text }}")
        myFixture.launchAction(action)
        return myFixture.editor.document.text
    }

    fun testValidGeneratorsAreClean() {
        val e = errors(
            """
            Iterator<int> upTo(int n) {
                for (int i = 0; i < n; i++) { yield i; }
                return;
            }
            Iterator<int> both(Iterator<int> a, Iterator<int> b) {
                yield* a;
                yield* b;
            }
            async Stream<int> ticks() { yield 1; }
            interface Source { static Iterator<int> of(int n) { yield n; } }
            """,
        )
        assertEquals(emptyList<String>(), e)
    }

    fun testYieldOutsideAGenerator() {
        val e = errors(
            """
            class A {
                public A() { yield 1; }
            }
            Iterator<int> f() {
                var g = () -> { yield 2; };
                yield 3;
            }
            int g(int t) { return switch (t) { default -> { yield 4; } }; }
            """,
        )
        assertEquals(e.toString(), 3, e.count { it.contains("E0990") })
    }

    fun testReturnValueInAGeneratorAndItsFix() {
        val e = errors("Iterator<int> f() { yield 1; return 2; }")
        assertTrue(e.toString(), e.any { it.contains("E0994") })
        val fixed = applyFix("Iterator<int> f() { yield 1; return 2; }", "Replace with 'yield 2;'")
        assertTrue(fixed, fixed.contains("yield 1; yield 2;"))
    }

    fun testGeneratorReturnTypeAndItsFix() {
        assertTrue(errors("int f() { yield 1; }").any { it.contains("E0996") })
        assertTrue(errors("async Iterator<int> f() { yield 1; }").any { it.contains("E0996") })
        assertEquals("Iterator<int> f() { yield 1; }", applyFix("int f() { yield 1; }", "Change return type to 'Iterator<int>'"))
        assertEquals(
            "async Stream<String> f() { yield \"a\"; }",
            applyFix("async void f() { yield \"a\"; }", "Change return type to 'Stream<String>'"),
        )
    }

    fun testInterfaceDefaultGeneratorAndUnsafeYield() {
        assertTrue(errors("interface S { Iterator<int> all() { yield 1; } }").any { it.contains("E0995") })
        assertTrue(errors("Iterator<int> f() { unsafe { yield 1; } }").any { it.contains("E0997") })
    }

    // ---- interface operator inspection --------------------------------------------

    fun testInterfaceOperatorShapes() {
        val e = errors(
            """
            interface Bad<T> {
                T operator+(T other) { return other; }
                bool operator==(T other);
                T operator*(T a, T b);
            }
            interface Good<T> { T operator-(T other); }
            class C {
                public C operator+(C other);
                public C operator-(C other) = delete;
            }
            """,
        )
        assertEquals(e.toString(), 4, e.count { it.contains("E0936") })
    }

    fun testRemoveInterfaceOperatorBody() {
        val fixed = applyFix("interface A<T> { T operator+(T other) { return other; } }", "Remove the operator's body")
        assertEquals("interface A<T> { T operator+(T other); }", fixed)
    }
}
