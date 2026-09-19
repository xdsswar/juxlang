package dev.jux.intellij.completion

import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.PlatformTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Completion and the structure view inside the syntax the compiler's Phase 3
 * added: a binding a new construct introduces is offered where it is in
 * scope, a receiver the new syntax produces completes its members, and the
 * new declarations show up in Ctrl+F12.
 */
class JuxPhase3CompletionTest : BasePlatformTestCase() {

    /**
     * The lookup strings offered at `<caret>`. A lone candidate is inserted
     * without a popup; that one is reported as the word left at the caret.
     */
    private fun offered(code: String): List<String> {
        myFixture.configureByText("a.jux", code)
        val items = myFixture.completeBasic() ?: return listOfNotNull(wordBeforeCaret())
        return items.map { it.lookupString }
    }

    private fun wordBeforeCaret(): String? {
        val text = myFixture.editor.document.charsSequence
        var start = myFixture.caretOffset
        while (start > 0 && (text[start - 1].isLetterOrDigit() || text[start - 1] == '_')) start--
        return text.subSequence(start, myFixture.caretOffset).toString().ifEmpty { null }
    }

    fun testRecordPatternBinderIsTypedByItsComponent() {
        val o = offered(
            """
            record Pt(int x, int y) {}
            record Line(Pt start, Pt end) {}
            int f(Object o) { return switch (o) { case Line(var s, var e) -> s.<caret>; default -> 0; }; }
            """.trimIndent(),
        )
        assertTrue("`s` binds Line.start, a Pt: $o", o.contains("x") && o.contains("y"))
    }

    fun testTypeTestBinderIsTyped() {
        val o = offered(
            """
            class Animal {}
            class Dog extends Animal { public void bark() {} }
            void main() { Animal pet = new Dog(); if (pet => Dog d) { d.<caret> } }
            """.trimIndent(),
        )
        assertTrue(o.toString(), o.contains("bark"))
    }

    fun testDestructuredBinderIsTypedByItsComponent() {
        val o = offered(
            """
            record Pt(int x, int y) {}
            record Line(Pt start, Pt end) {}
            void main() { var Line(var s, var e) = new Line(new Pt(1, 2), new Pt(3, 4)); s.<caret> }
            """.trimIndent(),
        )
        assertTrue(o.toString(), o.contains("x") && o.contains("y"))
    }

    fun testRecordPatternBinderInArmBody() {
        val o = offered(
            """
            sealed interface Shape {}
            record Circle(double radius) implements Shape {}
            double area(Shape s) { return switch (s) { case Circle(var rad) -> <caret>; default -> 0.0; }; }
            """.trimIndent(),
        )
        assertTrue("the pattern binder is in scope in its arm: $o", o.contains("rad"))
    }

    fun testDestructuredBindersAfterTheDeclaration() {
        val o = offered(
            """
            record Pt(int x, int y) {}
            void main() { var Pt(left, right) = new Pt(1, 2); print(<caret>); }
            """.trimIndent(),
        )
        assertTrue("both binders are offered: $o", o.contains("left") && o.contains("right"))
    }

    fun testTupleLocalMembers() {
        val o = offered("void main() { (int, String) pair = (1, \"a\"); int lenOfIt = 0; print(len<caret>); }")
        assertTrue(o.toString(), o.contains("lenOfIt"))
    }

    fun testEnumVariantCompletesEnumMethods() {
        val o = offered(
            """
            enum Planet {
                Earth(5.976e+24, 6.37814e6);
                private final double mass;
                private final double radius;
                Planet(double mass, double radius) { this.mass = mass; this.radius = radius; }
                public double gravity() { return mass / (radius * radius); }
            }
            void main() { print(Planet.Earth.<caret>); }
            """.trimIndent(),
        )
        assertTrue("an enum constant exposes its enum's methods: $o", o.contains("gravity"))
    }

    fun testEnumTypeOffersBuiltInLookups() {
        val o = offered(
            """
            enum Color { Red, Green }
            void main() { print(Color.<caret>); }
            """.trimIndent(),
        )
        assertTrue("variants: $o", o.contains("Red") && o.contains("Green"))
        assertTrue("built-in lookups: $o", o.contains("fromName") && o.contains("cases"))
    }

    fun testEnumLookupResultIsTheEnum() {
        val o = offered(
            """
            enum Color { Red, Green }
            void main() { var c = Color.fromOrdinal(0)!!; c.<caret> }
            """.trimIndent(),
        )
        assertTrue("a variant offers name() and ordinal(): $o", o.contains("name") && o.contains("ordinal"))
    }

    fun testPayloadEnumHasNoLookups() {
        val o = offered(
            """
            enum Reply { Ok(int status), Err(String why) }
            void main() { print(Reply.<caret>); }
            """.trimIndent(),
        )
        assertFalse("no fromName on a payload enum: $o", o.contains("fromName"))
        assertTrue("cases() is on every enum: $o", o.contains("cases"))
    }

    fun testMethodRefOnTypeOffersNew() {
        val o = offered(
            """
            record Point(int x, int y) { public static Point origin() { return new Point(0, 0); } }
            void main() { var f = Point::<caret>; }
            """.trimIndent(),
        )
        assertTrue(o.toString(), o.contains("new") && o.contains("origin"))
    }

    fun testInterfaceSuperCompletesInterfaceMethods() {
        val o = offered(
            """
            interface A { default String hi() { return "A"; } }
            class C implements A { public String hi() { return A.super.<caret>; } }
            """.trimIndent(),
        )
        assertTrue("Iface.super offers the interface's methods: $o", o.contains("hi"))
    }

    fun testBoundMethodRefCompletesMethods() {
        val o = offered(
            """
            class Greeter { public String greet(String who) { return who; } }
            void main() { var g = new Greeter(); (String) -> String f = g::<caret>; }
            """.trimIndent(),
        )
        assertTrue("obj:: offers the receiver's methods: $o", o.contains("greet"))
    }

    fun testSmartCastNarrowsCompletion() {
        val o = offered(
            """
            class Animal {}
            class Dog extends Animal { public void bark() {} }
            void main() { Animal pet = new Dog(); if (pet => Dog) { pet.<caret> } }
            """.trimIndent(),
        )
        assertTrue("the narrowed type's members are offered: $o", o.contains("bark"))
    }

    fun testDiamondNewCompletesMembers() {
        val o = offered(
            """
            class Box<T> { public T item; public Box(T item) { this.item = item; } public int size() { return 1; } }
            void main() { Box<int> b = new Box<>(3); b.<caret> }
            """.trimIndent(),
        )
        assertTrue(o.toString(), o.contains("size") && o.contains("item"))
    }

    fun testCompactCtorEnumCtorAndFreeOperatorInStructure() {
        myFixture.configureByText(
            "Shapes.jux",
            """
            record Range(int lo, int hi) {
                Range { if (lo > hi) { throw new IllegalArgumentException("x"); } }
            }
            enum Planet {
                Earth(1.0);
                private final double mass;
                Planet(double mass) { this.mass = mass; }
            }
            Range operator+(Range a, Range b) { return a; }
            """.trimIndent(),
        )
        myFixture.testStructureView { component ->
            PlatformTestUtil.expandAll(component.tree)
            PlatformTestUtil.assertTreeEqual(
                component.tree,
                """
                -Shapes.jux
                 -Range
                  Range
                 -Planet
                  Earth
                  mass
                  Planet
                 operator+
                """.trimIndent(),
            )
        }
    }

    fun testRemainingNewFormsParse() {
        val sources = mapOf(
            "asyncLambda" to "async int work() { return 1; }\nvoid main() { var f = async () -> { return await work(); }; }",
            "asyncFnType" to "async int twice(int x) { return x * 2; }\nasync int run(() async -> int job) { return await job(); }\n" +
                "void main() { () async -> int a = async () -> await twice(21); (int) async -> String d = async (int n) -> { return \"x\"; }; }",
            "boundRef" to "class G { public String greet(String w) { return w; } }\nvoid main() { var g = new G(); (String) -> String f = g::greet; }",
            "diamond" to "void main() { Vec<int> v = new Vec<>(); v.push(1); }",
            "tupleLocal" to "void main() { (int, String) t = (1, \"a\"); print(t.0); print(t.1); }",
            "exprBody" to "int twice(int x) = x * 2;",
            "labeledLoop" to "void main() { outer: for (int i = 0; i < 3; i++) { for (int j = 0; j < 3; j++) { if (j == 1) continue outer; } } }",
        )
        for ((name, text) in sources) {
            myFixture.configureByText("$name.jux", text)
            val errors = PsiTreeUtil.collectElementsOfType(myFixture.file, PsiErrorElement::class.java)
            assertTrue("$name: " + errors.joinToString { "${it.errorDescription} @${it.textOffset}" }, errors.isEmpty())
        }
    }
}
