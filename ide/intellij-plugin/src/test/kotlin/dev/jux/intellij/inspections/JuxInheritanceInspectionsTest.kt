package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The inheritance-shape inspections: E0429 missing implementations (+ the
 * "Implement methods" fix), the extends-clause rules (E0420/E0422/E0423 +
 * single inheritance), and implements-a-class (E0424) with the move fixes.
 * Unresolved supertypes must always stay silent.
 */
class JuxInheritanceInspectionsTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxAbstractNotImplementedInspection(),
            JuxExtendsClauseInspection(),
            JuxImplementsClauseInspection(),
            JuxInheritedTypeParamInspection(),
        )
    }

    private fun highlightDescriptions(code: String): List<String> {
        myFixture.configureByText("a.jux", code)
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    // ---- E0429 ---------------------------------------------------------------

    fun testMissingInterfaceMethodFlagged() {
        val d = highlightDescriptions(
            """
            public interface Shape {
                double area();
            }
            public class Circle implements Shape {
            }
            """.trimIndent(),
        )
        assertTrue(d.any { it.contains("E0429") && it.contains("'Shape.area'") })
    }

    fun testAbstractClassAndDefaultMethodExempt() {
        val d = highlightDescriptions(
            """
            public interface Shape {
                double area();
                default String label() { return "shape"; }
            }
            public abstract class Partial implements Shape {
            }
            public class Done implements Shape {
                public double area() { return 1.0; }
            }
            """.trimIndent(),
        )
        // Partial is abstract; Done implements area and inherits the default label.
        assertFalse(d.any { it.contains("E0429") })
    }

    fun testImplementMethodsQuickFix() {
        myFixture.configureByText(
            "a.jux",
            """
            public interface Shape {
                double area();
            }
            public class Cir<caret>cle implements Shape {
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val fix = myFixture.findSingleIntention("Implement methods")
        myFixture.launchAction(fix)
        assertTrue(myFixture.file.text.contains("public double area() {"))
        // The error clears after the fix.
        val after = myFixture.doHighlighting().mapNotNull { it.description }
        assertFalse(after.any { it.contains("E0429") })
    }

    // ---- extends clause --------------------------------------------------------

    fun testExtendsInterfaceFlaggedWithFix() {
        myFixture.configureByText(
            "a.jux",
            """
            public interface Walker {
                void walk();
            }
            public abstract class Robot extends Wal<caret>ker {
            }
            """.trimIndent(),
        )
        val d = myFixture.doHighlighting().mapNotNull { it.description }
        assertTrue(d.any { it.contains("E0423") && it.contains("an interface") })

        val fix = myFixture.findSingleIntention("Change to implements")
        myFixture.launchAction(fix)
        val text = myFixture.file.text
        assertTrue(text.contains("Robot implements Walker"))
        assertFalse(text.contains("Robot extends"))
    }

    fun testExtendsFinalFlagged() {
        val d = highlightDescriptions(
            """
            public final class Sealed {
            }
            public class Sub extends Sealed {
            }
            """.trimIndent(),
        )
        assertTrue(d.any { it.contains("E0420") })
    }

    fun testSealedPermitsRespected() {
        val d = highlightDescriptions(
            """
            public sealed class Base permits Good {
            }
            public class Good extends Base {
            }
            public class Rogue extends Base {
            }
            """.trimIndent(),
        )
        assertEquals(1, d.count { it.contains("E0422") })
    }

    fun testSecondExtendsEntryFlagged() {
        val d = highlightDescriptions(
            """
            public class A {
            }
            public class B {
            }
            public class C extends A, B {
            }
            """.trimIndent(),
        )
        assertTrue(d.any { it.contains("single inheritance") })
    }

    fun testInterfaceExtendingInterfacesIsClean() {
        val d = highlightDescriptions(
            """
            public interface A {
                void a();
            }
            public interface B {
                void b();
            }
            public interface C extends A, B {
            }
            """.trimIndent(),
        )
        assertFalse(d.any { it.contains("E04") })
    }

    fun testUnresolvedSupertypesStaySilent() {
        val d = highlightDescriptions(
            """
            public class Reader extends BufReader implements Display {
            }
            """.trimIndent(),
        )
        assertFalse(d.any { it.contains("E042") || it.contains("E0429") })
    }

    // ---- implements clause -------------------------------------------------------

    fun testImplementsClassFlaggedWithFix() {
        myFixture.configureByText(
            "a.jux",
            """
            public class Engine {
            }
            public class Car implements Eng<caret>ine {
            }
            """.trimIndent(),
        )
        val d = myFixture.doHighlighting().mapNotNull { it.description }
        assertTrue(d.any { it.contains("E0424") })

        val fix = myFixture.findSingleIntention("Move to extends")
        myFixture.launchAction(fix)
        val text = myFixture.file.text
        assertTrue(text.contains("Car extends Engine"))
        assertFalse(text.contains("Car implements"))
    }

    fun testGenericTypeArgumentIsNotFlaggedAsImplementedType() {
        // Regression: `implements Holder<Object>` has ONE supertype (Holder);
        // `Object` is a type ARGUMENT, not a separately-implemented type. A
        // recursive clause walk used to extract it and wrongly fire E0424.
        val d = highlightDescriptions(
            """
            public class Object {}
            public interface Holder<T> {
                void write(String name);
                void test(T t);
            }
            public class HolderName implements Holder<Object> {
                public void write(String name) {}
                public void test(Object t) {}
            }
            """.trimIndent(),
        )
        assertFalse("type argument Object must not be flagged (E0424)", d.any { it.contains("E0424") })
        assertFalse("Holder is implemented, nothing missing (E0429)", d.any { it.contains("E0429") })
    }

    fun testInheritedTypeParamWarnedAndFixedToBound() {
        myFixture.configureByText(
            "a.jux",
            """
            public interface Holder<T> {
                void test(T t);
            }
            public class HolderName implements Holder<Object> {
                public void test(T<caret> t) {}
            }
            """.trimIndent(),
        )
        val d = myFixture.doHighlighting().mapNotNull { it.description }
        assertTrue("warns T not declared, use Object", d.any { it.contains("'T'") && it.contains("'Object'") })

        val fix = myFixture.findSingleIntention("Replace with 'Object'")
        myFixture.launchAction(fix)
        val body = myFixture.file.text.substringAfter("implements Holder<Object> {")
        assertTrue("T replaced with Object", body.contains("public void test(Object t)"))
    }

    fun testDeclaredTypeParamNotWarned() {
        // Box DECLARES T, so forwarding it via `implements Holder<T>` is fine —
        // no inherited-param warning.
        val d = highlightDescriptions(
            """
            public interface Holder<T> { void test(T t); }
            public class Box<T> implements Holder<T> {
                public void test(T t) {}
            }
            """.trimIndent(),
        )
        assertFalse("declared T must not warn", d.any { it.contains("not declared here") })
    }

    fun testImplementsOnInterfaceFlagged() {
        val d = highlightDescriptions(
            """
            public interface A {
            }
            public interface B implements A {
            }
            """.trimIndent(),
        )
        assertTrue(d.any { it.contains("'implements' is not allowed") })
    }
}
