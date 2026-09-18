package dev.jux.intellij.parser

import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * The syntax the compiler's Phase 3 adds (switch patterns, record and enum
 * members, operators, smart casts) parses in the editor without a red
 * squiggle, and lands in the tree shape the rest of the plugin reads.
 */
class JuxPhase3SyntaxTest : BasePlatformTestCase() {

    private fun parses(name: String, text: String) {
        myFixture.configureByText("$name.jux", text)
        val errors = PsiTreeUtil.collectElementsOfType(myFixture.file, PsiErrorElement::class.java)
        assertTrue(
            "$name: " + errors.joinToString { "${it.errorDescription} @${it.textOffset}" },
            errors.isEmpty(),
        )
    }

    private fun count(type: com.intellij.psi.tree.IElementType): Int =
        PsiTreeUtil.collectElements(myFixture.file) { it.elementType === type }.size

    fun testSwitchPatterns() {
        parses("multi", "String f(Day d) { return switch (d) { case Sat, Sun -> \"we\"; default -> \"wd\"; }; }")
        parses("records", "double a(Shape s) { return switch (s) { case Circle(var r) -> r; case Sq(var x) -> x; }; }")
        parses("ranges", "String g(int n) { return switch (n) { case ..0 -> \"neg\"; case 0..10 -> \"s\"; case 10.. -> \"b\"; }; }")
        parses("negative", "String g(int n) { return switch (n) { case -5..=-1 -> \"n\"; case ..=-6 -> \"m\"; default -> \"p\"; }; }")
        parses("or", "int h(Shape s) { return switch (s) { case Circle(var v) | Sq(var v) -> 1; }; }")
    }

    fun testCompactRecordConstructorIsAConstructor() {
        parses(
            "compact",
            "record Range(int lo, int hi) { Range { if (lo > hi) { throw new IllegalArgumentException(\"x\"); } } Range(int single) { this(single, single); } }",
        )
        assertEquals(2, count(E.CONSTRUCTOR_DECLARATION))
    }

    fun testEnumFieldsAndModifiers() {
        parses(
            "planet",
            "enum Planet { Mercury(3.303e+23, 2.4397e6), Earth(5.976e+24, 6.37814e6); private final double mass; private final double radius; Planet(double mass, double radius) { this.mass = mass; this.radius = radius; } public double gravity() { return mass / (radius * radius); } }",
        )
        parses("modifiers", "final record R(int a) {}\nconst class K {}\nsealed enum E { A, B }")
    }

    fun testInterfacesAndOperators() {
        parses("ifaceSuper", "class C implements A, B { public String hi() { return A.super.hi() + B.super.hi(); } }")
        parses("ifaceProp", "interface Sized { int Size { get; } default bool isEmpty -> Size == 0; }")
        parses("freeOperator", "Vec3 operator*(double k, Vec3 v) { return v; }")
        assertEquals(1, count(E.OPERATOR_DECLARATION))
    }

    fun testDestructuringDeclaresEachBinder() {
        parses("record", "record Pt(int x, int y) {}\nvoid m(Pt p) { var Pt(a, b) = p; print(a + b); }")
        assertEquals(1, count(E.DESTRUCTURING_DECLARATION))
        assertEquals(2, count(E.LOCAL_VARIABLE))
        parses("nested", "void m(Line l) { var Line(Pt(x1, y1), var end) = l; var (i, j) = (1, 2); print(x1 + y1 + i + j); }")
        assertEquals(2, count(E.DESTRUCTURING_DECLARATION))
        assertEquals(5, count(E.LOCAL_VARIABLE))
    }

    fun testDestructuredBinderIsTheDeclarationOfItsUses() {
        myFixture.configureByText("go.jux", "record Pt(int x, int y) {}\nvoid m(Pt p) { var Pt(a, b) = p; print(<caret>a); }")
        val target = myFixture.elementAtCaret
        assertEquals(E.LOCAL_VARIABLE, target.elementType)
        assertEquals("a", (target as dev.jux.intellij.psi.JuxNamedElement).name)
        assertEquals(E.DESTRUCTURING_DECLARATION, target.parent.elementType)
    }

    fun testSmartCastForms() {
        parses("elvisThrow", "String need(String? s) { return s ?: throw new IllegalArgumentException(\"none\"); }")
        assertEquals(1, count(E.THROW_STATEMENT))
        parses("bareTypeTest", "void m(Animal a) { if (a => Dog) a.bark(); }")
        parses("assertStatement", "void m(int? x) { assert x != null; assert x > 0 : \"positive\"; assert(x != null); assert (x > 0) : \"p\"; }")
    }
}
