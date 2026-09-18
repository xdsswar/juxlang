package dev.jux.intellij.actions

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The Alt+Insert "Generate" actions: what each one writes, where it lands
 * (never inside another member, as with Java's), and when it is offered.
 */
class JuxGenerateActionsTest : BasePlatformTestCase() {

    private fun presentationVisible(action: AnAction): Boolean =
        myFixture.testAction(action).isEnabledAndVisible

    fun testEqualsAndHashFromEveryField() {
        myFixture.configureByText(
            "Person.jux",
            """
            public class Person {
                private String name;
                private int age;
                private double score;
                <caret>
            }
            """.trimIndent(),
        )
        myFixture.testAction(JuxGenerateEqualsAndHashAction())
        val text = myFixture.editor.document.text
        assertTrue(text, text.contains(
            """
                public bool operator==(Person other) {
                    return name == other.name && age == other.age && score == other.score;
                }
            """.trimIndent().prependIndent("    "),
        ))
        assertTrue(text, text.contains(
            """
                public int operator hash() {
                    int result = name.operator hash();
                    result = 31 *% result +% age.operator hash();
                    result = 31 *% result +% score.operator hash();
                    return result;
                }
            """.trimIndent().prependIndent("    "),
        ))
    }

    fun testEqualsAndHashSkipsComputedAndStaticMembers() {
        myFixture.configureByText(
            "Box.jux",
            """
            public class Box<T> {
                private static int created = 0;
                private T item;
                public int Weight { get; set; }
                public String Label -> "box";
                <caret>
            }
            """.trimIndent(),
        )
        myFixture.testAction(JuxGenerateEqualsAndHashAction())
        val text = myFixture.editor.document.text
        // The parameter is spelled with the type's own parameters.
        assertTrue(text, text.contains("public bool operator==(Box<T> other) {"))
        assertTrue(text, text.contains("return item == other.item && Weight == other.Weight;"))
        assertFalse(text, text.contains("created.operator hash()"))
        assertFalse(text, text.contains("Label.operator hash()"))
    }

    fun testEqualsAndHashAddsOnlyTheMissingHalf() {
        myFixture.configureByText(
            "Point.jux",
            """
            public class Point {
                private int x;
                public bool operator==(Point other) { return x == other.x; }
                <caret>
            }
            """.trimIndent(),
        )
        myFixture.testAction(JuxGenerateEqualsAndHashAction())
        val text = myFixture.editor.document.text
        assertEquals(text, 1, Regex("""operator==""").findAll(text).count())
        assertTrue(text, text.contains("int result = x.operator hash();"))
    }

    fun testEqualsAndHashOfferedOnlyOnAClassMissingIt() {
        myFixture.configureByText("Pt.jux", "public record Pt(int x, int y) { <caret> }")
        assertFalse(presentationVisible(JuxGenerateEqualsAndHashAction()))

        myFixture.configureByText(
            "Done.jux",
            """
            public class Done {
                private int x;
                public bool operator==(Done other) { return x == other.x; }
                public int operator hash() { return x.operator hash(); }
                <caret>
            }
            """.trimIndent(),
        )
        assertFalse(presentationVisible(JuxGenerateEqualsAndHashAction()))

        myFixture.configureByText("Open.jux", "public class Open { private int x; <caret> }")
        assertTrue(presentationVisible(JuxGenerateEqualsAndHashAction()))
    }

    fun testOperatorStringListsTheFields() {
        myFixture.configureByText(
            "Person.jux",
            """
            public class Person {
                private String name;
                private int age;
                <caret>
            }
            """.trimIndent(),
        )
        myFixture.testAction(JuxGenerateOperatorStringAction())
        val text = myFixture.editor.document.text
        assertTrue(text, text.contains(
            "    public String operator string() {\n" +
                "        return \$\"Person{name=\${name}, age=\${age}}\";\n" +
                "    }",
        ))
    }

    fun testOperatorStringNotOfferedTwice() {
        myFixture.configureByText(
            "Named.jux",
            """
            public class Named {
                public String operator string() { return "n"; }
                <caret>
            }
            """.trimIndent(),
        )
        assertFalse(presentationVisible(JuxGenerateOperatorStringAction()))
    }

    fun testGeneratedMemberLandsAfterTheMethodHoldingTheCaret() {
        myFixture.configureByText(
            "Walker.jux",
            """
            public class Walker {
                private int steps;
                public void walk() {
                    steps<caret> = steps + 1;
                }
            }
            """.trimIndent(),
        )
        myFixture.testAction(JuxGenerateOperatorStringAction())
        val text = myFixture.editor.document.text
        // The method body is untouched, and the new member follows it.
        assertTrue(text, text.contains("        steps = steps + 1;\n    }\n"))
        assertTrue(text, text.indexOf("operator string") > text.indexOf("public void walk()"))
        assertTrue(text, text.indexOf("operator string") > text.indexOf("steps + 1;\n    }"))
    }
}
