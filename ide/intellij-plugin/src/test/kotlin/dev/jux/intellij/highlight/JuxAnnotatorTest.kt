package dev.jux.intellij.highlight

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Semantic-highlighting coverage: decl-vs-reference colouring from
 * [JuxAnnotator] and string-interior colouring from [JuxStringAnnotator].
 * Asserts on the forced text-attribute keys the annotators attach, found by
 * the exact text range of an identifier occurrence.
 */
class JuxAnnotatorTest : BasePlatformTestCase() {

    /** Keys of every annotation that exactly covers occurrence [n] of [token]. */
    private fun keysAt(code: String, token: String, n: Int = 1): Set<String> {
        myFixture.configureByText("a.jux", code)
        var idx = -1
        repeat(n) { idx = code.indexOf(token, idx + 1) }
        check(idx >= 0) { "token '$token' (#$n) not found in test code" }
        val start = idx
        val end = idx + token.length
        return myFixture.doHighlighting()
            .filter { it.startOffset == start && it.endOffset == end }
            .mapNotNull { it.forcedTextAttributesKey?.externalName }
            .toSet()
    }

    private val demo = """
        package demo;

        public class Greeter<T> {
            private String name;

            public String greet(String who) {
                var msg = "Hi";
                helper(msg);
                return who;
            }

            public void helper(String s) {}

            public T pass(T item) { return item; }
        }

        public enum Color { Red, Green }
    """.trimIndent()

    fun testLocalVariableReadIsColored() {
        // `msg` use inside helper(msg) — second occurrence.
        assertContainsElements(keysAt(demo, "msg", 2), "JUX_LOCAL_VARIABLE")
    }

    fun testParameterReadIsColored() {
        // `who` in `return who;` — second occurrence.
        assertContainsElements(keysAt(demo, "who", 2), "JUX_PARAMETER")
    }

    fun testCallSiteIsColored() {
        assertContainsElements(keysAt(demo, "helper", 1), "JUX_METHOD_CALL")
    }

    fun testTypeParameterUseIsColored() {
        // The `T` in `T pass(...)`'s return position (a TYPE_REFERENCE use).
        assertContainsElements(keysAt(demo, "T pass", 1).ifEmpty { keysAt(demo, "T", 2) }, "JUX_TYPE_PARAMETER")
    }

    fun testEnumConstantDeclarationIsColored() {
        assertContainsElements(keysAt(demo, "Red", 1), "JUX_ENUM_CONSTANT")
    }

    fun testClassNameInTypePositionIsColored() {
        // `String` is a primitive-style name → TYPE; a user class name should
        // get CLASS_NAME even when it only resolves cross-file.
        val code = """
            package demo;
            public class Holder {
                public void take(Beast b) {}
            }
        """.trimIndent()
        assertContainsElements(keysAt(code, "Beast", 1), "JUX_CLASS_NAME")
    }

    // ---- string interiors --------------------------------------------------

    fun testValidEscapeIsColored() {
        val code = """
            package demo;
            public class S { public String x = "a\nb"; }
        """.trimIndent()
        assertContainsElements(keysAt(code, "\\n", 1), "JUX_VALID_ESCAPE")
    }

    fun testInvalidEscapeIsColored() {
        val code = """
            package demo;
            public class S { public String x = "a\qb"; }
        """.trimIndent()
        assertContainsElements(keysAt(code, "\\q", 1), "JUX_INVALID_ESCAPE")
    }

    fun testInterpolationDelimitersAndInterior() {
        val code = """
            package demo;
            public class S {
                public void p(int count) {
                    var s = ${'$'}"total = ${'$'}{count + 1}";
                }
            }
        """.trimIndent()
        assertContainsElements(keysAt(code, "\${", 1), "JUX_INTERPOLATION")
        // The interior is re-lexed: `count` is an identifier token.
        assertContainsElements(keysAt(code, "count + 1", 1).ifEmpty { keysAt(code, "count", 2) }, "JUX_IDENTIFIER")
    }

    fun testRawStringHasNoEscapeColoring() {
        val code = """
            package demo;
            public class S { public String x = ${"\"\"\""}a\nb${"\"\"\""}; }
        """.trimIndent()
        assertEmpty(keysAt(code, "\\n", 1).filter { it.endsWith("ESCAPE") })
    }
}
