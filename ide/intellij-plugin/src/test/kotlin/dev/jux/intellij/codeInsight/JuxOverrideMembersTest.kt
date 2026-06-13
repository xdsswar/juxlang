package dev.jux.intellij.codeInsight

import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * The shared implement/override engine behind Ctrl+I / Ctrl+O / Alt+Insert:
 * candidate classification (abstract → IMPLEMENT, bodied → OVERRIDE),
 * name+arity dedupe, modifier exclusions, cycle safety, and the inserted stub
 * shapes (`@override` + throw vs `super.…` delegation).
 */
class JuxOverrideMembersTest : BasePlatformTestCase() {

    private fun typeNamed(name: String): JuxTypeDeclaration =
        PsiTreeUtil.findChildrenOfType(myFixture.file, JuxTypeDeclaration::class.java)
            .first { it.name == name }

    fun testClassificationImplementVsOverride() {
        myFixture.configureByText(
            "a.jux",
            """
            public interface Greeter {
                String greet(String who);
                default void wave() { print("wave"); }
            }
            public class Base {
                public int size() { return 0; }
                private void hidden() {}
                public static void util() {}
                public final void locked() {}
            }
            public class Impl extends Base implements Greeter {
            }
            """.trimIndent(),
        )
        val candidates = JuxOverrideMembers.candidates(typeNamed("Impl"))
        val byName = candidates.associateBy { it.method.name }

        assertEquals(JuxOverrideMembers.Kind.IMPLEMENT, byName["greet"]?.kind)
        assertEquals(JuxOverrideMembers.Kind.OVERRIDE, byName["wave"]?.kind) // default body
        assertEquals(JuxOverrideMembers.Kind.OVERRIDE, byName["size"]?.kind)
        // static / private / final never show up.
        assertNull(byName["hidden"])
        assertNull(byName["util"])
        assertNull(byName["locked"])
    }

    fun testOwnDeclarationsAndNearestWinExcluded() {
        myFixture.configureByText(
            "a.jux",
            """
            public interface Shape {
                double area();
                String label();
            }
            public abstract class Mid implements Shape {
                public double area() { return 1.0; }
            }
            public class Leaf extends Mid {
                public String label() { return "leaf"; }
            }
            """.trimIndent(),
        )
        val candidates = JuxOverrideMembers.candidates(typeNamed("Leaf"))
        // label: declared by Leaf itself → excluded entirely.
        assertFalse(candidates.any { it.method.name == "label" })
        // area: abstract in Shape but implemented by the nearer Mid → OVERRIDE.
        val area = candidates.single { it.method.name == "area" }
        assertEquals(JuxOverrideMembers.Kind.OVERRIDE, area.kind)
        assertEquals("Mid", area.ownerName)
    }

    fun testOverloadsAreSeparateByArity() {
        myFixture.configureByText(
            "a.jux",
            """
            public interface Adder {
                int add(int a);
                int add(int a, int b);
            }
            public class A implements Adder {
                public int add(int a) { return a; }
            }
            """.trimIndent(),
        )
        val candidates = JuxOverrideMembers.candidates(typeNamed("A"))
        // The 1-arg overload is declared; only the 2-arg one remains.
        val adds = candidates.filter { it.method.name == "add" }
        assertEquals(1, adds.size)
        assertTrue(adds.single().signature.contains(","))
    }

    fun testExtendsCycleTerminates() {
        myFixture.configureByText(
            "a.jux",
            """
            public class A extends B { }
            public class B extends A { }
            """.trimIndent(),
        )
        // Just must not hang / overflow.
        assertEmpty(JuxOverrideMembers.candidates(typeNamed("A")))
    }

    fun testInsertedStubShapes() {
        myFixture.configureByText(
            "a.jux",
            """
            public interface Greeter {
                String greet(String who);
            }
            public class Base {
                public void log(String msg) { print(msg); }
                public int count() { return 0; }
            }
            public class Impl extends Base implements Greeter {
                public void own() {}
            }
            """.trimIndent(),
        )
        val type = typeNamed("Impl")
        JuxOverrideMembers.insertStubs(project, type, JuxOverrideMembers.candidates(type))
        val text = myFixture.file.text

        // IMPLEMENT stub: @override + UnsupportedOperationException body.
        assertTrue(text.contains("@override"))
        assertTrue(text.contains("public String greet(String who) {"))
        assertTrue(text.contains("throw new UnsupportedOperationException(\"TODO: greet\");"))
        // OVERRIDE stubs: delegate to super, `return` only when non-void.
        assertTrue(text.contains("super.log(msg);"))
        assertFalse(text.contains("return super.log"))
        assertTrue(text.contains("return super.count();"))
        // Stubs land inside Impl's body (after `own`, before the final `}`).
        assertTrue(text.indexOf("greet(String who) {", text.indexOf("class Impl")) > text.indexOf("own()"))
    }

    fun testImplementMethodsPlatformAction() {
        myFixture.configureByText(
            "a.jux",
            """
            public interface Runner {
                void run();
            }
            public class R implements Runner {
                <caret>
            }
            """.trimIndent(),
        )
        // Unit-test mode auto-selects all candidates (no chooser dialog).
        myFixture.performEditorAction("ImplementMethods")
        val text = myFixture.file.text
        assertTrue(text.contains("public void run() {"))
        assertTrue(text.contains("@override"))
    }

    fun testOverrideMethodsPlatformAction() {
        myFixture.configureByText(
            "a.jux",
            """
            public class Base {
                public int value() { return 1; }
            }
            public class Sub extends Base {
                <caret>
            }
            """.trimIndent(),
        )
        myFixture.performEditorAction("OverrideMethods")
        val text = myFixture.file.text
        assertTrue(text.contains("return super.value();"))
    }
}
