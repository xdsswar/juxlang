package dev.jux.intellij.completion

import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.codeInsight.lookup.LookupElementPresentation
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Member completion after `.`, owned by the plugin: what a receiver offers,
 * how each item reads (return type in the type column, parameters as tail
 * text), where the caret lands on accept, and in which order.
 */
class JuxMemberCompletionTest : BasePlatformTestCase() {

    private fun items(code: String): List<LookupElement> {
        myFixture.configureByText("a.jux", code)
        return myFixture.completeBasic()?.toList() ?: emptyList()
    }

    private fun names(code: String) = items(code).map { it.lookupString }

    private fun presentation(item: LookupElement) = LookupElementPresentation.renderElement(item)

    fun testInheritedMembersAndInterfaceDefaultsAreOfferedBelowOwnMembers() {
        val o = names(
            """
            public interface Named { String label() { return "n"; } }
            public class Base { public int baseCount() { return 1; } }
            public class Item extends Base implements Named { public int ownCount() { return 2; } }
            void main() { Item i = new Item(); i.<caret> }
            """.trimIndent(),
        )
        assertTrue("an interface default is a member: $o", "label" in o)
        assertTrue("an inherited method is a member: $o", "baseCount" in o)
        assertTrue("own before inherited: $o", o.indexOf("ownCount") < o.indexOf("baseCount"))
    }

    fun testStubTypeMembersAreOffered() {
        myFixture.addFileToProject(
            ".jux-stubs/rust/demo.jux.d",
            "package rust.demo;\n\npublic class Widget {\n    public int width();\n    public void resize(int w);\n}\n",
        )
        val o = names("import rust.demo.*;\nvoid main() { Widget w = new Widget(); w.<caret> }")
        assertTrue("stub members: $o", o.containsAll(listOf("width", "resize")))
    }

    fun testStaticMembersAfterATypeName() {
        val o = names(
            """
            public class Config {
                public static int limit() { return 1; }
                public static int LEVEL = 2;
                public int instanceOnly() { return 3; }
            }
            void main() { Config.<caret> }
            """.trimIndent(),
        )
        assertTrue("statics after a type name: $o", o.containsAll(listOf("limit", "LEVEL")))
        assertFalse("no instance member after a type name: $o", "instanceOnly" in o)
    }

    fun testEnumVariantsAfterAnEnumName() {
        val o = names(
            """
            public enum Color { RED, GREEN, BLUE }
            void main() { Color.<caret> }
            """.trimIndent(),
        )
        assertTrue("variants: $o", o.containsAll(listOf("RED", "GREEN", "BLUE")))
    }

    fun testRecordComponentsReadAsProperties() {
        val all = items(
            """
            public record Point(int x, int y) { }
            void main() { Point p = new Point(1, 2); p.<caret> }
            """.trimIndent(),
        )
        val x = all.first { it.lookupString == "x" }
        assertEquals("int", presentation(x).typeText)
    }

    fun testNamedOperatorsOnEveryValue() {
        val o = names(
            """
            public class Point { public int x = 0; }
            void main() { Point p = new Point(); p.<caret> }
            """.trimIndent(),
        )
        assertTrue("operator string and hash on a class value: $o", o.containsAll(listOf("operator string", "operator hash")))
        assertTrue("a type's own members rank above them: $o", o.indexOf("x") < o.indexOf("operator string"))
    }

    fun testNamedOperatorOnAPrimitiveInsertsTheCall() {
        // Typing the operator's name alone finds it.
        myFixture.configureByText("a.jux", "void main() { int n = 1; n.stri<caret> }")
        myFixture.completeBasic()
        myFixture.checkResult("void main() { int n = 1; n.operator string()<caret> }")
    }

    fun testArraysHaveNoOperatorHash() {
        val o = names("void main() { int[] xs = new int[3]; xs.<caret> }")
        assertFalse("an array has no operator hash (E0933): $o", "operator hash" in o)
    }

    fun testMethodShowsReturnTypeAndParameters() {
        val all = items(
            """
            public class Calc { public int add(int a, int b) { return a + b; } public int zero() { return 0; } }
            void main() { Calc c = new Calc(); c.<caret> }
            """.trimIndent(),
        )
        val add = presentation(all.first { it.lookupString == "add" })
        assertEquals("int", add.typeText)
        assertEquals("(int a, int b)", add.tailText)
    }

    fun testNoArgumentMethodPutsTheCaretAfterTheParentheses() {
        myFixture.configureByText(
            "a.jux",
            "public class Calc { public int zero() { return 0; } }\nvoid main() { Calc c = new Calc(); c.zer<caret> }",
        )
        myFixture.completeBasic()
        myFixture.checkResult(
            "public class Calc { public int zero() { return 0; } }\nvoid main() { Calc c = new Calc(); c.zero()<caret> }",
        )
    }

    fun testMethodWithArgumentsPutsTheCaretInside() {
        myFixture.configureByText(
            "a.jux",
            "public class Calc { public int add(int a, int b) { return a + b; } }\nvoid main() { Calc c = new Calc(); c.ad<caret> }",
        )
        myFixture.completeBasic()
        myFixture.checkResult(
            "public class Calc { public int add(int a, int b) { return a + b; } }\nvoid main() { Calc c = new Calc(); c.add(<caret>) }",
        )
    }

    fun testChainedReceiverThroughAGenericGetter() {
        val o = names(
            """
            public class Truck { public int load() { return 1; } }
            public class Box<T> { T item; public T get() { return item; } }
            void main() { Box<Truck> b = new Box<Truck>(); b.get().<caret> }
            """.trimIndent(),
        )
        assertTrue("members of the type argument: $o", "load" in o)
    }
}
