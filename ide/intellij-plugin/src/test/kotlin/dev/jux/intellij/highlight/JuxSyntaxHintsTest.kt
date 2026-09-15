package dev.jux.intellij.highlight

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Alt+Enter on broken code: [JuxSyntaxHintAnnotator] names slips that parse
 * but are never meant, and [dev.jux.intellij.editor.JuxSyntaxQuickFixProvider]
 * repairs the syntax error the parser reported. Each case applies the fix and
 * checks the resulting text.
 */
class JuxSyntaxHintsTest : BasePlatformTestCase() {

    private fun applyFix(code: String, fix: String): String {
        myFixture.configureByText("Main.jux", code.trimIndent())
        myFixture.doHighlighting()
        myFixture.launchAction(myFixture.findSingleIntention(fix))
        return myFixture.file.text
    }

    fun testAssignmentInCondition() {
        val text = applyFix(
            """
            void main() {
                int x = 1;
                if (x =<caret> 5) { print(x); }
            }
            """,
            "Replace '=' with '=='",
        )
        assertTrue(text, text.contains("if (x == 5)"))
    }

    fun testMultiCharCharLiteralBecomesString() {
        val text = applyFix(
            """
            void main() {
                print('ab<caret>c');
            }
            """,
            "Convert to a String literal",
        )
        assertTrue(text, text.contains("print(\"abc\")"))
    }

    fun testFatArrowLambda() {
        val text = applyFix(
            """
            void main() {
                var f = x =<caret>> x * 2;
            }
            """,
            "Replace '=>' with '->'",
        )
        assertTrue(text, text.contains("x -> x * 2"))
    }

    fun testLetBecomesVar() {
        val text = applyFix(
            """
            void main() {
                l<caret>et x = 3;
            }
            """,
            "Replace 'let' with 'var'",
        )
        assertTrue(text, text.contains("var x = 3;"))
    }

    fun testInsertMissingSemicolonAfterTheLastToken() {
        val text = applyFix(
            """
            void main() {
                int x = 1<caret>
                print(x);
            }
            """,
            "Insert ';'",
        )
        assertTrue(text, text.contains("int x = 1;\n"))
    }

    fun testJavaCaseColon() {
        val text = applyFix(
            """
            int f(int n) {
                return switch (n) {
                    case 1<caret>: 10;
                    default -> 0;
                };
            }
            """,
            "Replace ':' with '->'",
        )
        assertTrue(text, text.contains("case 1 -> 10;"))
    }

    fun testRemoveStrayParen() {
        val text = applyFix(
            """
            void main() {
                print(1)<caret>);
            }
            """,
            "Remove ')'",
        )
        assertTrue(text, text.contains("print(1);"))
    }

    fun testUnclosedStringIsAnError() {
        myFixture.configureByText("Main.jux", "void main() {\n    print(\"abc);\n}\n")
        val errors = myFixture.doHighlighting(HighlightSeverity.ERROR).map { it.description }
        assertTrue(errors.toString(), "Illegal line end in string literal" in errors)
    }

    fun testIdiomaticCodeGetsNoHints() {
        myFixture.configureByText(
            "Main.jux",
            """
            class Shape {}
            class Circle extends Shape {}
            void main() {
                int x = 1;
                if (x == 1) { print('a'); }
                while ((x = x + 1) < 3) { print("x"); }
                Shape s = new Circle();
                if (s => Circle c) { print(c); }
                var f = (int y) -> y * 2;
                char nl = '\n';
                /* closed */
            }
            """.trimIndent(),
        )
        val hints = setOf(
            "Illegal line end in string literal", "Unclosed character literal",
            "Too many characters in character literal", "Empty character literal", "Unclosed comment",
            "Assignment in condition; did you mean '=='?", "Lambdas use '->'; '=>' is the type-test operator",
            "Jux declares a local with 'var', not 'let'", "Jux writes 'else if', not 'elif'",
        )
        val found = myFixture.doHighlighting().mapNotNull { it.description }.filter { it in hints }
        assertTrue("no hints on valid code: $found", found.isEmpty())
    }
}
