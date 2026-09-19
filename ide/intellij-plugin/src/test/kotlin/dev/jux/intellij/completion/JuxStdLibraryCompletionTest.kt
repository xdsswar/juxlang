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

    fun testStdMembersNavigateAndDocument() {
        myFixture.configureByText(
            "a.jux",
            """
            void main() {
                Option<String> opt = Option.Some("x");
                var n = opt.m<caret>ap((s) -> s.len());
            }
            """.trimIndent(),
        )
        val target = myFixture.file.findReferenceAt(myFixture.caretOffset)?.resolve()
        assertNotNull("Option.map resolves into the bundled jux.std", target)
        assertTrue(target!!.containingFile.name, target.containingFile.name == "Option.jux")
        val doc = dev.jux.intellij.documentation.JuxDocumentationProvider().generateDoc(target, null) ?: ""
        assertTrue(doc, doc.contains("jux.std.option.Option"))
        assertTrue(doc, doc.contains("map"))
    }

    fun testMutexAndIteratorCombinatorsComplete() {
        val m = names(
            """
            void main() {
                var lock = new Mutex<int>(0);
                lock.<caret>
            }
            """,
        )
        assertTrue("lock in $m", "lock" in m)
        val it = names(
            """
            Iterator<int> numbers() {
                yield 1;
            }

            void main() {
                numbers().<caret>
            }
            """,
        )
        for (c in listOf("map", "filter", "take", "skip", "zip", "chain", "reduce", "count", "any", "all", "firstOrNull")) {
            assertTrue("$c in $it", c in it)
        }
    }

    fun testAbiNamesComplete() {
        val expr = names(
            """
            void main() {
                var n = al<caret>
            }
            """,
        )
        assertTrue("alignof in $expr", "alignof" in expr)
        val ann = names(
            """
            @<caret>
            struct Wide {
                long v;
            }
            """,
        )
        assertTrue("align in $ann", "align" in ann)
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
