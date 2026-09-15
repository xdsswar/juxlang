package dev.jux.intellij.parser

import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.ParsingTestCase
import dev.jux.intellij.psi.JuxElementTypes
import dev.jux.intellij.psi.JuxParserDefinition

/**
 * What the editor says about broken code while it is being typed.
 *
 * Each case is one common mistake. The bar is IntelliJ's Java editor: one
 * message, at the mistake, in words that say what to do -- and the rest of the
 * file stays parsed, so one missing brace does not paint every member after it
 * red or empty the structure view.
 */
class JuxSyntaxErrorsTest : ParsingTestCase("", "jux", JuxParserDefinition()) {

    override fun getTestDataPath(): String = java.io.File("../../examples").absolutePath

    private data class Err(val message: String, val line: Int, val column: Int)

    private fun errors(code: String): List<Err> {
        val file = createPsiFile("e.jux", code.trimIndent())
        val text = file.text
        return PsiTreeUtil.collectElementsOfType(file, PsiErrorElement::class.java).map { e ->
            val offset = e.textRange.startOffset
            val before = text.substring(0, offset)
            val line = before.count { it == '\n' } + 1
            val column = offset - (before.lastIndexOf('\n') + 1) + 1
            Err(e.errorDescription, line, column)
        }
    }

    private fun assertOne(code: String, message: String, line: Int) {
        val errs = errors(code)
        assertEquals("one error expected, got $errs", 1, errs.size)
        assertEquals("message: $errs", message, errs[0].message)
        assertEquals("line: $errs", line, errs[0].line)
    }

    fun testMissingSemicolonIsOneErrorOnItsLine() {
        assertOne(
            """
            void main() {
                int x = 1
                print(x);
            }
            """,
            "';' expected",
            2,
        )
    }

    fun testMissingMethodBraceDoesNotSwallowTheNextMembers() {
        val code = """
            class A {
                public void first() {
                    print(1);

                public void second() { print(2); }
                public void third() { print(3); }
            }
        """.trimIndent()
        val errs = errors(code)
        assertEquals("one error for one missing brace: $errs", 1, errs.size)
        assertEquals("'}' expected", errs[0].message)
        val file = createPsiFile("m.jux", code)
        val methods = PsiTreeUtil.findChildrenOfType(file, com.intellij.psi.PsiElement::class.java)
            .count { it.node.elementType === JuxElementTypes.METHOD_DECLARATION }
        assertEquals("all three methods are still members", 3, methods)
    }

    fun testElseWithoutIf() {
        assertOne(
            """
            void main() {
                print(1);
                else { print(2); }
            }
            """,
            "'else' without 'if'",
            3,
        )
    }

    fun testCatchWithoutTry() {
        assertOne(
            """
            void main() {
                print(1);
                catch (Exception e) { print(2); }
            }
            """,
            "'catch' without 'try'",
            3,
        )
    }

    fun testStrayClosingParen() {
        assertOne(
            """
            void main() {
                print(1));
            }
            """,
            "Unexpected ')'",
            2,
        )
    }

    fun testIfWithoutParentheses() {
        val errs = errors(
            """
            void main() {
                int x = 1;
                if x > 0 {
                    print(x);
                }
            }
            """,
        )
        assertTrue("at most two errors: $errs", errs.size in 1..2)
        assertEquals("'(' expected", errs[0].message)
        assertEquals(3, errs[0].line)
    }

    fun testJavaStyleCaseColon() {
        val errs = errors(
            """
            int f(int n) {
                return switch (n) {
                    case 1: 10;
                    default -> 0;
                };
            }
            """,
        )
        assertEquals("one error: $errs", 1, errs.size)
        assertEquals("'->' expected", errs[0].message)
        assertEquals(3, errs[0].line)
    }

    fun testMissingCommaBetweenArguments() {
        val errs = errors(
            """
            void main() {
                add(1 2);
            }
            """,
        )
        assertEquals("one error: $errs", 1, errs.size)
        assertEquals("',' or ')' expected", errs[0].message)
    }

    fun testMissingExpressionIsReported() {
        assertOne(
            """
            void main() {
                int x = ;
            }
            """,
            "Expression expected",
            2,
        )
    }

    fun testMissingOperandIsReported() {
        assertOne(
            """
            void main() {
                int x = 3 + ;
            }
            """,
            "Expression expected",
            2,
        )
    }

    fun testUnclosedGenericStopsAtTheDeclaration() {
        val errs = errors(
            """
            class A {
                private Vec<int items = new Vec<int>();
                public void go() { print(1); }
            }
            """,
        )
        assertTrue("reported at the declaration, not at the end of the file: $errs", errs.isNotEmpty() && errs[0].line == 2)
        assertEquals("'>' expected", errs[0].message)
        assertTrue("the next member is untouched: $errs", errs.none { it.line >= 3 })
    }

    fun testUnclosedArraySizeStopsAtTheStatement() {
        val errs = errors(
            """
            void main() {
                var a = new int[3;
                print(a);
            }
            """,
        )
        assertTrue("reported on line 2: $errs", errs.isNotEmpty() && errs[0].line == 2)
        assertEquals("']' expected", errs[0].message)
        assertTrue("no errors after the statement: $errs", errs.none { it.line >= 3 })
    }

    fun testNewerJavaShapesParseClean() {
        val errs = errors(
            """
            public sealed interface Shape permits Circle, Square {}
            public record Circle(double r) implements Shape {}
            public String kind(char c) {
                return switch (c) {
                    case 'a' | 'e' -> "vowel";
                    case 'x'..='z' -> "late";
                    default -> throw new IllegalStateException("bad");
                };
            }
            void main() {
                char[] xs = {
                    'a',
                    'b',
                };
                if (xs != null && xs.length > 0) { print(xs[0]); }
            }
            """,
        )
        assertTrue("no errors: $errs", errs.isEmpty())
    }

    fun testValidCodeStaysClean() {
        val errs = errors(
            """
            class A {
                public int n = 1;
                public void go(int x) {
                    if (x > 0) { print(x); } else { print(-x); }
                    var list = new Vec<int>();
                    list.push(n * 2);
                    int y = switch (x) { case 1 -> 10; default -> 0; };
                    try { print(y); } catch (Exception e) { print(e); }
                }
            }
            """,
        )
        assertTrue("no errors: $errs", errs.isEmpty())
    }
}
