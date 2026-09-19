package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Plugin 0.1.3: the comparator rule (Operators §O.2.1), lambdas assigned into
 * interface-typed slots (LANG-V1 §7.9.1), function-type variance (Type
 * system §T.3.6), refinements not carried into lambdas (§T.6.4), and the
 * builtins that now come from the lexer's generated lists.
 */
class JuxLambdaTypingWaveTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxComparatorResultInspection(),
            JuxFunctionTypeVarianceInspection(),
            JuxNullableAccessInspection(),
            JuxUnresolvedReferenceInspection(),
        )
    }

    private fun descriptions(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    private fun completions(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        myFixture.completeBasic()
        return myFixture.lookupElementStrings ?: emptyList()
    }

    /** A Rust-shaped sort slot, spelled the way the rust.std stub spells it. */
    private val sorter = """
        enum Ordering { Less, Equal, Greater }

        class People {
            public void sort_unstable_by((Person, Person) -> Ordering compare) {}
        }

        record Person(String name, int age) {}
    """

    // ---- comparators --------------------------------------------------------

    fun testIntegerComparatorIsFine() {
        val d = descriptions(
            sorter + """
            public void main() {
                var people = new People();
                people.sort_unstable_by((a, b) -> a.age - b.age);
                people.sort_unstable_by((a, b) -> { return b.age - a.age; });
            }
            """,
        )
        assertFalse(d.toString(), d.any { "comparator" in it })
    }

    fun testOrderingComparatorIsFine() {
        val d = descriptions(
            sorter + """
            public void main() {
                var people = new People();
                people.sort_unstable_by((a, b) -> Ordering.Less);
            }
            """,
        )
        assertFalse(d.toString(), d.any { "comparator" in it })
    }

    fun testStringComparatorIsReported() {
        val d = descriptions(
            sorter + """
            public void main() {
                var people = new People();
                people.sort_unstable_by((a, b) -> a.name);
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.startsWith("a comparator returns an `int` or an `Ordering`, found String") })
    }

    // ---- assigned lambdas (§7.9.1) -------------------------------------------

    fun testAssignedLambdaParametersAreTypedFromTheInterface() {
        val items = completions(
            """
            interface Pricer { long price(Item item, int qty); }
            record Item(String name, long cents) {}

            public void main() {
                Pricer p = null;
                p = (item, qty) -> item.<caret>;
            }
            """,
        )
        assertTrue(items.toString(), "cents" in items)
        assertTrue(items.toString(), "name" in items)
    }

    fun testLambdaAssignedToAFieldIsTypedFromTheField() {
        val items = completions(
            """
            interface Listener { void on(Event e); }
            record Event(String kind, int code) {}

            class Bus {
                private Listener handler;
                Bus() {
                    this.handler = (e) -> print(e.<caret>);
                }
            }
            """,
        )
        assertTrue(items.toString(), "kind" in items)
    }

    // ---- function-type variance (§T.3.6) ------------------------------------

    fun testVarianceWrongWayIsReported() {
        val d = descriptions(
            """
            class Animal { public String name() { return "animal"; } }
            class Dog extends Animal { public String bark() { return "woof"; } }

            public void main() {
                (Dog) -> String dogsOnly = (d) -> d.bark();
                (Animal) -> String anyAnimal = dogsOnly;

                () -> Animal makeAnimal = () -> new Animal();
                () -> Dog makeDog = makeAnimal;
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.startsWith("type mismatch in declaration of `anyAnimal`: expected (Animal) -> String, found (Dog) -> String") })
        assertTrue(d.toString(), d.any { it.startsWith("type mismatch in declaration of `makeDog`: expected () -> Dog, found () -> Animal") })
    }

    fun testVarianceRightWayIsClean() {
        val d = descriptions(
            """
            class Animal { public String name() { return "animal"; } }
            class Dog extends Animal { public String bark() { return "woof"; } }

            public void main() {
                (Animal) -> String anyAnimal = (a) -> a.name();
                (Dog) -> String dogsToo = anyAnimal;

                () -> Dog makeDog = () -> new Dog();
                () -> Animal makeAnimal = makeDog;
                print(dogsToo(new Dog()) + makeAnimal().name());
            }
            """,
        )
        assertFalse(d.toString(), d.any { it.startsWith("type mismatch") })
    }

    fun testCallThroughAFunctionTypedLocalHasTheResultType() {
        val items = completions(
            """
            record Item(String name, long cents) {}

            public void main() {
                (int) -> Item make = (n) -> new Item("x", n);
                make(1).<caret>
            }
            """,
        )
        assertTrue(items.toString(), "cents" in items)
    }

    // ---- refinements do not enter lambdas (§T.6.4) ---------------------------

    fun testCheckOutsideALambdaDoesNotHoldInsideIt() {
        val d = descriptions(
            """
            record User(String name) {}

            void later(() -> String f) { print(f()); }

            public void main() {
                User? maybe = new User("a");
                if (maybe != null) {
                    later(() -> maybe.name);
                }
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.startsWith("'maybe' may be null") })
    }

    fun testFreshLocalInsideTheLambdaIsClean() {
        val d = descriptions(
            """
            record User(String name) {}

            void later(() -> String f) { print(f()); }

            public void main() {
                User? maybe = new User("a");
                later(() -> {
                    var u = maybe;
                    if (u == null) { return "none"; }
                    return u.name;
                });
            }
            """,
        )
        assertFalse(d.toString(), d.any { it.startsWith("'u' may be null") || it.startsWith("'maybe' may be null") })
    }

    fun testCheckOutsideWithoutALambdaStillCounts() {
        val d = descriptions(
            """
            record User(String name) {}

            public void main() {
                User? maybe = new User("a");
                if (maybe != null) {
                    print(maybe.name);
                }
            }
            """,
        )
        assertFalse(d.toString(), d.any { it.startsWith("'maybe' may be null") })
    }

    // ---- builtins from the generated lists ----------------------------------

    fun testNeverAndRangeTypesNeedNoImport() {
        val d = descriptions(
            """
            never fail(String why) { throw new IllegalStateException(why); }

            public void main() {
                ExclusiveRange<int> r = 0..3;
                InclusiveRange<int> s = 0..=3;
                SteppedRange<int> t = 0..10 step 2;
                print(r.start + s.end + t.step);
            }
            """,
        )
        assertFalse(d.toString(), d.any { it.startsWith("Cannot resolve symbol") })
    }

    fun testAlignIsOfferedAfterAt() {
        myFixture.configureByText("a.jux", "@al<caret>\nstruct Block { public int x = 0; }\n")
        val items = myFixture.completeBasic()?.map { it.lookupString }
        // A single candidate is inserted straight away.
        val offered = items?.any { it.equals("align", ignoreCase = true) }
            ?: myFixture.editor.document.text.startsWith("@align")
        assertTrue(items.toString() + " / " + myFixture.editor.document.text, offered)
    }
}
