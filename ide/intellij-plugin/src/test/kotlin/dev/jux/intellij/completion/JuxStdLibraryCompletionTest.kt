package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The bundled `jux.std` sources (the compiler's embedded standard library)
 * and the typing built on them: `Option`/`Result`/`Iterator` members complete
 * with their real signatures, an untyped lambda parameter gets its type from
 * the function type or single-method interface it is given to, a range value
 * has its components, and a for-each over a map binds a `(K, V)` tuple.
 */
class JuxStdLibraryCompletionTest : BasePlatformTestCase() {

    private fun names(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    fun testOptionCombinatorsComplete() {
        val o = names(
            """
            void main() {
                Option<String> opt = Option.Some("x");
                opt.<caret>
            }
            """,
        )
        for (m in listOf("map", "flatMap", "orElse", "filter", "unwrapOrElse", "isSomeAnd", "toNullable")) {
            assertTrue("$m in $o", m in o)
        }
    }

    fun testResultCombinatorsComplete() {
        val o = names(
            """
            void main(Result<int, String> r) {
                r.<caret>
            }
            """,
        )
        for (m in listOf("map", "mapErr", "flatMap", "orElse", "isOkAnd", "unwrapOrElse")) {
            assertTrue("$m in $o", m in o)
        }
    }

    fun testLambdaParameterTakesTheOptionElementType() {
        val o = names(
            """
            class Order {
                public int total = 0;
                public int quantity() { return 1; }
            }

            void main() {
                Option<Order> opt = Option.Some(new Order());
                var q = opt.map((p) -> p.<caret>);
            }
            """,
        )
        assertTrue("members of Order in $o", "total" in o && "quantity" in o)
    }

    fun testLambdaForASingleMethodInterface() {
        val o = names(
            """
            class Order {
                public int id = 0;
            }

            interface Listener<T> {
                void onEvent(T value);
            }

            class Bus {
                public void subscribe(Listener<Order> l) {}
            }

            void main() {
                var bus = new Bus();
                bus.subscribe((o) -> print(o.<caret>));
            }
            """,
        )
        assertTrue("id in $o", "id" in o)
    }

    fun testLambdaForADeclaredFunctionType() {
        val o = names(
            """
            class Point {
                public int x = 0;
            }

            void main() {
                (Point) -> int getX = (p) -> p.<caret>;
            }
            """,
        )
        assertTrue("x in $o", "x" in o)
    }

    fun testRangeComponents() {
        val exclusive = names(
            """
            void main() {
                var r = 1..5;
                print(r.<caret>);
            }
            """,
        )
        assertTrue("start/end in $exclusive", "start" in exclusive && "end" in exclusive)
        val stepped = names(
            """
            void main() {
                var evens = 0..10 step 2;
                print(evens.<caret>);
            }
            """,
        )
        assertTrue("step in $stepped", "step" in stepped)
        val inclusive = names(
            """
            void main() {
                var dice = 1..=6;
                print(dice.<caret>);
            }
            """,
        )
        assertTrue("endInclusive in $inclusive", "endInclusive" in inclusive)
    }

    fun testMapForEachBindsATuple() {
        myFixture.addFileToProject(
            ".jux-stubs/rust/std.jux.d",
            "package rust.std;\n\npublic class HashMap<K, V> {\n    public void insert(K k, V v);\n}\n",
        )
        val o = names(
            """
            import rust.std.HashMap;

            class Order {
                public int id = 0;
            }

            void main() {
                var orders = new HashMap<String, Order>();
                for (var e : orders) {
                    print(e.<caret>);
                }
            }
            """,
        )
        assertTrue("0 and 1 in $o", "0" in o && "1" in o)
        val v = names(
            """
            import rust.std.HashMap;

            class Order {
                public int id = 0;
            }

            void main() {
                var orders = new HashMap<String, Order>();
                for (var e : orders) {
                    print(e.1.<caret>);
                }
            }
            """,
        )
        assertTrue("id in $v", "id" in v)
    }
}
