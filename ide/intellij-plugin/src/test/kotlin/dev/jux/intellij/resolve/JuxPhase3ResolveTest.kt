package dev.jux.intellij.resolve

import com.intellij.psi.util.elementType
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/**
 * Go-to-declaration on the syntax the compiler's Phase 3 added: each case puts
 * the caret on a use and checks it lands on the declaration it names.
 */
class JuxPhase3ResolveTest : BasePlatformTestCase() {

    /** Resolve the reference at `<caret>` and return the target's name and element type. */
    private fun target(text: String): Pair<String?, Any?> {
        myFixture.configureByText("a.jux", text)
        val ref = myFixture.file.findReferenceAt(myFixture.caretOffset)
        val resolved = ref?.resolve()
        return (resolved as? JuxNamedElement)?.name to resolved?.elementType
    }

    fun testEnumVariantReceiverReachesEnumMethods() {
        val (name, type) = target(
            """
            enum Planet {
                Earth(5.976e+24, 6.37814e6);
                private final double mass;
                private final double radius;
                Planet(double mass, double radius) { this.mass = mass; this.radius = radius; }
                public double gravity() { return mass / (radius * radius); }
            }
            void main() { print(Planet.Earth.grav<caret>ity()); }
            """.trimIndent(),
        )
        assertEquals("gravity", name)
        assertEquals(E.METHOD_DECLARATION, type)
    }

    fun testEnumVariantResolves() {
        val (name, type) = target(
            """
            enum Planet {
                Earth(5.976e+24, 6.37814e6);
                private final double mass;
                Planet(double mass, double radius) { this.mass = mass; }
            }
            void main() { var p = Planet.Ea<caret>rth; }
            """.trimIndent(),
        )
        assertEquals("Earth", name)
        assertEquals(E.ENUM_CONSTANT, type)
    }

    fun testSealedEnumPermitsNamesItsVariants() {
        val (name, type) = target("sealed enum Signal permits Red, Am<caret>ber { Red, Amber }")
        assertEquals("Amber", name)
        assertEquals(E.ENUM_CONSTANT, type)
    }

    fun testConstGenericParameter() {
        val (name, type) = target("class Ring<int N> { public int[] slots = new int[<caret>N]; }")
        assertEquals("N", name)
        assertEquals(E.TYPE_PARAMETER, type)
    }

    fun testFieldOfNewExpression() {
        val (name, _) = target(
            """
            class Config { public int retries = 3; }
            void main() { print(new Config().ret<caret>ries); }
            """.trimIndent(),
        )
        assertEquals("retries", name)
    }

    fun testNestedTypeInNewExpression() {
        val (name, type) = target(
            """
            class Shapes { public record Point(int x, int y) {} }
            void main() { print(new Shapes.Po<caret>int(1, 2)); }
            """.trimIndent(),
        )
        assertEquals("Point", name)
        assertEquals(E.RECORD_DECLARATION, type)
    }

    fun testComponentOfNestedRecord() {
        val (name, type) = target(
            """
            class Shapes { public record Point(int x, int y) {} }
            void main() { print(new Shapes.Point(1, 2).<caret>x); }
            """.trimIndent(),
        )
        assertEquals("x", name)
        assertEquals(E.RECORD_COMPONENT, type)
    }

    fun testBoundMethodReference() {
        val (name, type) = target(
            """
            class Greeter { public String greet(String who) { return who; } }
            void main() { var g = new Greeter(); (String) -> String f = g::gr<caret>eet; }
            """.trimIndent(),
        )
        assertEquals("greet", name)
        assertEquals(E.METHOD_DECLARATION, type)
    }

    fun testConstructorReference() {
        val (name, _) = target(
            """
            record Point(int x, int y) {}
            void main() { (int, int) -> Point at = Po<caret>int::new; }
            """.trimIndent(),
        )
        assertEquals("Point", name)
    }

    fun testSmartCastNarrowsTheReceiver() {
        val (name, type) = target(
            """
            class Animal {}
            class Dog extends Animal { public void bark() {} }
            void main() { Animal pet = new Dog(); if (pet => Dog) pet.ba<caret>rk(); }
            """.trimIndent(),
        )
        assertEquals("bark", name)
        assertEquals(E.METHOD_DECLARATION, type)
    }

    fun testSmartCastInBlockBody() {
        val (name, _) = target(
            """
            class Animal {}
            class Dog extends Animal { public void bark() {} }
            void main() { Animal pet = new Dog(); if (pet => Dog) { pet.ba<caret>rk(); } }
            """.trimIndent(),
        )
        assertEquals("bark", name)
    }

    fun testFieldThroughMemberChain() {
        val (name, _) = target(
            """
            class Money { public int total = 0; }
            class Wallet { public Money money = new Money(); }
            void main() { var wallet = new Wallet(); print(wallet.money.to<caret>tal); }
            """.trimIndent(),
        )
        assertEquals("total", name)
    }

    fun testDestructuredBinderResolves() {
        val (name, type) = target(
            """
            record Pt(int x, int y) {}
            void main() { var p = new Pt(1, 2); var Pt(a, b) = p; print(<caret>a); }
            """.trimIndent(),
        )
        assertEquals("a", name)
        assertEquals(E.LOCAL_VARIABLE, type)
    }

    fun testLabeledBreakResolvesToItsLabel() {
        myFixture.configureByText("a.jux", "void main() { outer: { if (true) { break ou<caret>ter; } } }")
        val ref = myFixture.file.findReferenceAt(myFixture.caretOffset)
        val resolved = ref?.resolve()
        assertNotNull("break label resolves", resolved)
        assertEquals(E.LABELED_STATEMENT, resolved?.elementType)
    }

    fun testRenameLabelRewritesItsBreaks() {
        myFixture.configureByText(
            "a.jux",
            "void main() { out<caret>er: for (int i = 0; i < 3; i++) { if (i == 1) break outer; continue outer; } }",
        )
        myFixture.renameElement(myFixture.elementAtCaret, "scan")
        myFixture.checkResult("void main() { scan: for (int i = 0; i < 3; i++) { if (i == 1) break scan; continue scan; } }")
    }

    fun testFindUsagesOfLabel() {
        myFixture.configureByText(
            "a.jux",
            "void main() { out<caret>er: { if (true) { break outer; } break outer; } }",
        )
        assertEquals(2, myFixture.findUsages(myFixture.elementAtCaret).size)
    }

    fun testLabelDoesNotLeakIntoALambda() {
        myFixture.configureByText(
            "a.jux",
            "void main() { outer: { () -> void f = () -> { while (true) { break ou<caret>ter; } }; } }",
        )
        assertNull(myFixture.file.findReferenceAt(myFixture.caretOffset)?.resolve())
    }

    fun testRenameDestructuredBinder() {
        myFixture.configureByText(
            "a.jux",
            "record Pt(int x, int y) {}\nvoid main() { var p = new Pt(1, 2); var Pt(a<caret>, b) = p; print(a + b); }",
        )
        myFixture.renameElement(myFixture.elementAtCaret, "left")
        myFixture.checkResult("record Pt(int x, int y) {}\nvoid main() { var p = new Pt(1, 2); var Pt(left, b) = p; print(left + b); }")
    }

    fun testRecordPatternBinderResolves() {
        val (name, type) = target(
            """
            sealed interface Shape {}
            record Circle(double r) implements Shape {}
            double area(Shape s) { return switch (s) { case Circle(var rad) -> 3.0 * ra<caret>d; default -> 0.0; }; }
            """.trimIndent(),
        )
        assertEquals("rad", name)
        assertEquals(E.LOCAL_VARIABLE, type)
    }

    fun testTypePatternBinderResolvesInGuard() {
        val (name, _) = target(
            """
            class Animal {}
            class Dog extends Animal { public bool loud() { return true; } }
            String f(Animal a) { return switch (a) { case Dog d when <caret>d.loud() -> "loud"; default -> "quiet"; }; }
            """.trimIndent(),
        )
        assertEquals("d", name)
    }

    fun testTypeTestBinderResolvesInThenBranch() {
        val (name, type) = target(
            """
            class Animal {}
            class Dog extends Animal { public void bark() {} }
            void main() { Animal a = new Dog(); if (a => Dog d && <caret>d != null) { d.bark(); } }
            """.trimIndent(),
        )
        assertEquals("d", name)
        assertEquals(E.LOCAL_VARIABLE, type)
    }

    fun testTypeTestBinderMethodResolves() {
        val (name, _) = target(
            """
            class Animal {}
            class Dog extends Animal { public void bark() {} }
            void main() { Animal a = new Dog(); if (a => Dog d) { d.ba<caret>rk(); } }
            """.trimIndent(),
        )
        assertEquals("bark", name)
    }

    fun testInterfaceSuperCallResolves() {
        val (name, type) = target(
            """
            interface A { default String hi() { return "A"; } }
            class C implements A { public String hi() { return A.super.h<caret>i(); } }
            """.trimIndent(),
        )
        assertEquals("hi", name)
        assertEquals(E.METHOD_DECLARATION, type)
    }

    fun testRenamePatternBinder() {
        myFixture.configureByText(
            "a.jux",
            "record C(double r) {}\ndouble f(Object o) { return switch (o) { case C(var r<caret>ad) -> rad * rad; default -> 0.0; }; }",
        )
        myFixture.renameElement(myFixture.elementAtCaret, "radius")
        myFixture.checkResult(
            "record C(double r) {}\ndouble f(Object o) { return switch (o) { case C(var radius) -> radius * radius; default -> 0.0; }; }",
        )
    }

    fun testFreeOperatorIsAFunctionInStructure() {
        myFixture.configureByText("a.jux", "record V(int x) {}\nV operator*(int k, V v) { return new V(k * v.x); }")
        val ops = com.intellij.psi.util.PsiTreeUtil.collectElements(myFixture.file) { it.elementType === E.OPERATOR_DECLARATION }
        assertEquals(1, ops.size)
    }
}
