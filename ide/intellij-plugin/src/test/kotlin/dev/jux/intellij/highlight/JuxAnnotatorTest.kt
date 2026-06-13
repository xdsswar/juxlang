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

    // ---- observable properties (§P.5 native coloring) ----------------------

    private val propsDemo = """
        package demo;

        public class Model {
            public String Name { get; set; } = "";
            public String Id { get; private set; } = "";
            private String test { get; set; } = "t";

            private final observer<String> nameObs = (old, now) -> {
                print(now);
            };

            public void wire(Model m) {
                Name.observers.attach(nameObs);
                Name.observers.clear;
                print(Name.observers.size);
                m.Name.observers.attach(nameObs);
                Name.bind(Id);
                Name.unbind();
                test.bind(Id);
                m.Title.bindBidirectional(Id);
            }

            public void notProps() {
                var bind = "hello";
                print(bind);
                myObject.attach(x);
                helper.clear();
            }
        }
    """.trimIndent()

    private val inheritedGenericsDemo = """
        package demo;
        public interface Holder<T> {
            void test(T t);
            T getIt();
        }
        public class HolderName implements Holder<Object> {
            public void test(T t) {}
            public T getIt() { return null; }
        }
    """.trimIndent()

    fun testInheritedTypeParameterIsColoredAsTypeParameter() {
        // Jux ruling: `T` (Holder's parameter) is usable in HolderName because
        // it implements `Holder<Object>` — so it must color as a TYPE PARAMETER
        // (green), not an unresolved type. Capital-`T` occurrences in order:
        // 1 `Holder<T>`, 2 interface `test(T t)`, 3 interface `T getIt`,
        // 4 class `test(T t)`, 5 class `T getIt` — assert the class ones (4, 5).
        assertContainsElements(keysAt(inheritedGenericsDemo, "T", 4), "JUX_TYPE_PARAMETER")
        assertContainsElements(keysAt(inheritedGenericsDemo, "T", 5), "JUX_TYPE_PARAMETER")
    }

    fun testObserverTypeIsPrimitiveColored() {
        assertContainsElements(keysAt(propsDemo, "observer", 1), "JUX_TYPE")
    }

    fun testObserversMemberIsPrimitiveColored() {
        // `observers` is colored like `observer` / a primitive (TYPE), so the
        // built-in §P member reads as language vocabulary, not a user field.
        assertContainsElements(keysAt(propsDemo, "observers", 1), "JUX_TYPE")
        // Cross-object receiver (`m.Name.observers`) colors via the heuristic.
        assertContainsElements(keysAt(propsDemo, "observers", 4), "JUX_TYPE")
    }

    fun testObserversOpsAreNativeColored() {
        assertContainsElements(keysAt(propsDemo, "attach", 1), "JUX_NATIVE_OPERATION")
        assertContainsElements(keysAt(propsDemo, "clear", 1), "JUX_NATIVE_OPERATION")
        assertContainsElements(keysAt(propsDemo, "size", 1), "JUX_NATIVE_OPERATION")
    }

    fun testBindOpsAreNativeColored() {
        // NOTE on occurrence counting: the substring "bind" also matches inside
        // `unbind` and at the head of `bindBidirectional` — occurrences are
        // counted over raw text, hence the explicit indexes below.
        // #1 = `Name.bind(Id)` — resolved PascalCase property receiver.
        assertContainsElements(keysAt(propsDemo, "bind", 1), "JUX_NATIVE_OPERATION")
        assertContainsElements(keysAt(propsDemo, "unbind", 1), "JUX_NATIVE_OPERATION")
        // #3 = `test.bind(Id)` — in-file camelCase property receiver resolves.
        assertContainsElements(keysAt(propsDemo, "bind", 3), "JUX_NATIVE_OPERATION")
        // Unresolved PascalCase receiver (`m.Title`) colors via the convention.
        assertContainsElements(keysAt(propsDemo, "bindBidirectional", 1), "JUX_NATIVE_OPERATION")
    }

    fun testNonPropertyUsesStayPlain() {
        // `print(bind)` reading the local `bind` — never native-colored
        // ("bind" #6: after bind(Id), un[bind], test.[bind], [bind]Bidirectional, var [bind]).
        assertEmpty(keysAt(propsDemo, "bind", 6).filter { it.startsWith("JUX_NATIVE") })
        // `myObject.attach(x)` ("attach" #3) — no `.observers` receiver → plain call.
        assertEmpty(keysAt(propsDemo, "attach", 3).filter { it.startsWith("JUX_NATIVE") })
        // `helper.clear()` ("clear" #2) — parens on a non-observers receiver → plain call.
        assertEmpty(keysAt(propsDemo, "clear", 2).filter { it.startsWith("JUX_NATIVE") })
    }

    fun testSetterValueIsParameterColored() {
        val code = """
            package demo;
            public class C {
                private int _age;
                public int Age {
                    get -> _age;
                    set { _age = value; }
                };
            }
        """.trimIndent()
        assertContainsElements(keysAt(code, "value", 1), "JUX_PARAMETER")
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
        // The interior is re-lexed: a bare identifier read inside a hole is a
        // variable, so `count` (occurrence #2 — #1 is the `int count` param)
        // gets the interpolated-variable colour, not the plain identifier one.
        assertContainsElements(keysAt(code, "count", 2), "JUX_INTERPOLATED_VARIABLE")
    }

    fun testInterpolationShorthandNameIsColored() {
        val code = """
            package demo;
            public class S {
                public void p(String who) {
                    var s = ${'$'}"hi ${'$'}who";
                }
            }
        """.trimIndent()
        // `$who` shorthand: the `$` is a delimiter, `who` (#2 — #1 is the param)
        // is an interpolated variable.
        assertContainsElements(keysAt(code, "who", 2), "JUX_INTERPOLATED_VARIABLE")
    }

    fun testInterpolationCallNameStaysPlain() {
        val code = """
            package demo;
            public class S {
                public void p(int x) {
                    var s = ${'$'}"v = ${'$'}{fmt(x)}";
                }
            }
        """.trimIndent()
        // A call NAME inside a hole is not a variable: `fmt` keeps the plain
        // identifier colour (next non-ws char is `(`)…
        assertContainsElements(keysAt(code, "fmt", 1), "JUX_IDENTIFIER")
        assertDoesntContain(keysAt(code, "fmt", 1), "JUX_INTERPOLATED_VARIABLE")
        // …while the argument `x` (#2 — #1 is the param) is still a variable.
        assertContainsElements(keysAt(code, "x", 2), "JUX_INTERPOLATED_VARIABLE")
    }

    fun testRawStringHasNoEscapeColoring() {
        val code = """
            package demo;
            public class S { public String x = ${"\"\"\""}a\nb${"\"\"\""}; }
        """.trimIndent()
        assertEmpty(keysAt(code, "\\n", 1).filter { it.endsWith("ESCAPE") })
    }
}
