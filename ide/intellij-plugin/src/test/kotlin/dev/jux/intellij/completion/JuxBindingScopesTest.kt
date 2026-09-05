package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Bindings the parser used to swallow.
 *
 * A for-each variable, a C-`for` init variable, a catch parameter and a lambda
 * parameter were consumed as raw tokens with no node at all, and a type
 * parameter had a node but no named PSI. So none of them could be completed,
 * navigated to, renamed, or counted as used — the four things that follow from
 * being a declaration.
 *
 * These are also the bindings that appear in the most-typed code in any
 * language, which is why their absence was so visible: the loop variable you
 * just wrote never appearing in the popup.
 */
class JuxBindingScopesTest : BasePlatformTestCase() {

    private fun offered(code: String): List<String> {
        myFixture.configureByText("a.jux", code)
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    fun testForEachVariableIsInScope() {
        val o = offered(
            """
            void main() {
                Vec<int> xs = new Vec<int>();
                for (int item : xs) {
                    print(<caret>);
                }
            }
            """.trimIndent(),
        )
        assertTrue("the loop variable is in scope in its body: $o", o.contains("item"))
    }

    fun testCForInitVariableIsInScope() {
        val o = offered(
            """
            void main() {
                for (int i = 0; i < 3; i = i + 1) {
                    print(<caret>);
                }
            }
            """.trimIndent(),
        )
        assertTrue("the init variable is in scope in the body: $o", o.contains("i"))
    }

    fun testCatchParameterIsInScope() {
        val o = offered(
            """
            void main() {
                try {
                    print("x");
                } catch (Exception ex) {
                    print(<caret>);
                }
            }
            """.trimIndent(),
        )
        assertTrue("the caught exception is in scope: $o", o.contains("ex"))
    }

    fun testLambdaParameterIsInScope() {
        val o = offered(
            """
            void main() {
                Vec<int> xs = new Vec<int>();
                xs.iter().map((n) -> <caret>);
            }
            """.trimIndent(),
        )
        assertTrue("a lambda parameter is in scope in its body: $o", o.contains("n"))
    }

    fun testTypeParameterIsInScope() {
        val o = offered(
            """
            public class Box<T> {
                private T value;
                public void set(<caret>) { }
            }
            """.trimIndent(),
        )
        assertTrue("the class's type parameter is a usable type: $o", o.contains("T"))
    }

    /** A binding must not leak past the construct that introduced it. */
    fun testBindingsDoNotLeakOutOfTheirScope() {
        val o = offered(
            """
            void main() {
                for (int item : xs) { print(item); }
                print(<caret>);
            }
            """.trimIndent(),
        )
        assertFalse("the loop variable is gone after the loop: $o", o.contains("item"))
    }
}
