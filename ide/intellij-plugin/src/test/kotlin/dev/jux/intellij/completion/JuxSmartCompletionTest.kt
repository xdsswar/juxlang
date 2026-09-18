package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.CompletionType
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Smart completion (Ctrl+Shift+Space): only what fits the type the caret
 * wants, plus what that type itself provides (`new T()`, an enum's variants,
 * `null`, `true`/`false`).
 */
class JuxSmartCompletionTest : BasePlatformTestCase() {

    private fun smart(code: String): List<String> {
        myFixture.configureByText("a.jux", code)
        return myFixture.complete(CompletionType.SMART)?.map { it.lookupString } ?: emptyList()
    }

    fun testOnlyFittingLocalsAndANewInstance() {
        val o = smart(
            """
            public class Shape { }
            void main() {
                Shape existing = new Shape();
                int count = 1;
                Shape s = <caret>
            }
            """.trimIndent(),
        )
        assertTrue("a fitting local: $o", "existing" in o)
        assertTrue("a new instance of the wanted class: $o", "new Shape" in o)
        assertFalse("an int does not fit a Shape: $o", "count" in o)
        assertFalse("keywords are not values of Shape: $o", "return" in o)
    }

    fun testEnumVariantsForAnEnumSlot() {
        val o = smart(
            """
            public enum Color { RED, GREEN }
            void main() { Color c = <caret> }
            """.trimIndent(),
        )
        assertTrue("qualified variants: $o", o.containsAll(listOf("Color.RED", "Color.GREEN")))
        assertFalse("an enum cannot be constructed: $o", "new Color" in o)
    }

    fun testNullOnlyForANullableSlot() {
        val nullable = smart("public class Shape { }\nvoid main() { Shape? s = <caret> }")
        assertTrue("null fits a nullable slot: $nullable", "null" in nullable)
        val plain = smart("public class Shape { }\nvoid main() { Shape s = <caret> }")
        assertFalse("null does not fit a non-null slot: $plain", "null" in plain)
    }

    fun testBoolSlotOffersLiteralsAndBoolValues() {
        val o = smart(
            """
            void main() {
                bool ready = true;
                int n = 0;
                bool ok = <caret>
            }
            """.trimIndent(),
        )
        assertTrue("literals: $o", o.containsAll(listOf("true", "false")))
        assertTrue("a bool local: $o", "ready" in o)
        assertFalse("an int: $o", "n" in o)
    }

    fun testAbstractClassesAreNotInstantiated() {
        val o = smart(
            """
            public abstract class Shape { }
            public class Circle extends Shape { }
            void main() { Circle c = new Circle(); Shape s = <caret> }
            """.trimIndent(),
        )
        assertFalse("no new of an abstract class: $o", "new Shape" in o)
        assertTrue("a subclass value fits: $o", "c" in o)
    }

    fun testMembersAfterADotAreFilteredByType() {
        val o = smart(
            """
            public class Shape { }
            public class Factory {
                public Shape make() { return new Shape(); }
                public Shape copy() { return new Shape(); }
                public int count() { return 0; }
            }
            void main() { Factory f = new Factory(); Shape s = f.<caret> }
            """.trimIndent(),
        )
        assertTrue("the Shape-returning method: $o", "make" in o)
        assertFalse("the int one: $o", "count" in o)
    }

    fun testStaticFactoriesOfTheWantedType() {
        val o = smart(
            """
            public class Config {
                public static Config defaults() { return new Config(); }
                public static int version() { return 1; }
            }
            void main() { Config c = <caret> }
            """.trimIndent(),
        )
        assertTrue("a static factory: $o", "Config.defaults" in o)
        assertFalse("a static that gives an int: $o", "Config.version" in o)
    }

    fun testNewInstanceInsertsParenthesesWithTheCaretInsideForArguments() {
        myFixture.configureByText(
            "a.jux",
            "public record Point(int x, int y) { }\nvoid main() { Point p = <caret> }",
        )
        val items = myFixture.complete(CompletionType.SMART)
        if (items != null) {
            val item = items.first { it.lookupString == "new Point" }
            myFixture.lookup.currentItem = item
            myFixture.finishLookup(com.intellij.codeInsight.lookup.Lookup.NORMAL_SELECT_CHAR)
        }
        myFixture.checkResult("public record Point(int x, int y) { }\nvoid main() { Point p = new Point(<caret>) }")
    }
}
