package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * E0446 for a bound a foreign method puts on its own type's parameters
 * (`@RustBounds("T: Ord")`, Bindgen §G.6.4.4, ERRATA E77).
 *
 * The fixture stub is shaped exactly like the generated `rust.std` one: the
 * annotations sit in front of the member, and `Vec` carries `@RustCollection`.
 * The reading under test is the backend's: `Ord`/`PartialOrd` is
 * `operator<=>`, a float element meets `PartialOrd` but not `Ord`, and an
 * array or a collection element meets neither.
 */
class JuxForeignBoundsInspectionTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(JuxForeignBoundsInspection())
        myFixture.addFileToProject(".jux-stubs/rust/std.jux.d", STUB)
    }

    private fun descriptions(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    fun testSortOnAClassWithNoSpaceshipIsReported() {
        val d = descriptions(
            """
            import rust.std.*;
            public class Person {
                public int age = 0;
            }
            void main() {
                Vec<Person> people = new Vec<Person>();
                people.sort();
            }
            """,
        )
        assertTrue(
            "E0446 naming the element type: $d",
            d.any { it.contains("E0446") && it.contains("Person") && it.contains("operator<=>") },
        )
    }

    fun testSortOnAClassThatDeclaresSpaceshipIsSilent() {
        val d = descriptions(
            """
            import rust.std.*;
            public class Person {
                public int age = 0;
                public int operator<=>(Person other) { return 0; }
            }
            void main() {
                Vec<Person> people = new Vec<Person>();
                people.sort();
            }
            """,
        )
        assertTrue("a declared `operator<=>` satisfies Ord: $d", d.none { it.contains("E0446") })
    }

    fun testSortOnPrimitiveElementsIsSilent() {
        val d = descriptions(
            """
            import rust.std.*;
            void main() {
                Vec<int> numbers = new Vec<int>();
                numbers.sort();
                Vec<String> names = new Vec<String>();
                names.sort();
            }
            """,
        )
        assertTrue("the primitives and String are ordered: $d", d.none { it.contains("E0446") })
    }

    fun testSortOnFloatElementsIsReportedForTheTotalOrder() {
        val d = descriptions(
            """
            import rust.std.*;
            void main() {
                Vec<double> values = new Vec<double>();
                values.sort();
            }
            """,
        )
        assertTrue("a double has no TOTAL order: $d", d.any { it.contains("E0446") && it.contains("NaN") })
    }

    fun testSortByTakesAComparatorAndCarriesNoOrdBound() {
        val d = descriptions(
            """
            import rust.std.*;
            public class Person {
                public int age = 0;
            }
            void main() {
                Vec<Person> people = new Vec<Person>();
                people.sort_by((a, b) -> a.age <=> b.age);
            }
            """,
        )
        assertTrue("`sort_by` declares no bound, so nothing fires: $d", d.none { it.contains("E0446") })
    }

    fun testTypeParameterElementIsNotReported() {
        val d = descriptions(
            """
            import rust.std.*;
            public class Box<T> {
                public void sortAll(Vec<T> items) { items.sort(); }
            }
            """,
        )
        // A type parameter is checked where it is instantiated (§T.4), never
        // at the declaration that only passes it along.
        assertTrue("a type parameter is not the place to report: $d", d.none { it.contains("E0446") })
    }

    fun testACallOnTheUsersOwnTypeIsNeverReported() {
        val d = descriptions(
            """
            public class Person {}
            public class Roster<T> {
                public void sort() {}
            }
            void main() {
                Roster<Person> r = new Roster<Person>();
                r.sort();
            }
            """,
        )
        assertTrue("only a stub declares @RustBounds: $d", d.none { it.contains("E0446") })
    }

    private companion object {
        /** A slice of the generated `rust.std` stub, written exactly as the emitter writes it. */
        val STUB = """
            package rust.std;

            @rust("std::vec::Vec")
            @RustClone
            @RustCollection
            public class Vec<T, A> {
                @MutSelf public void push(T value);
                public uint len();
                @MutSelf @RustBounds("T: Ord") public void sort();
                @MutSelf @RustClosureRefs("0") public void sort_by<F>((T, T) -> int compare);
                @RustBounds("T: Clone") public Vec<T> to_vec();
                @RustBounds("T: Ord") public uint binary_search(T x);
                @MutSelf public void reverse();
            }
        """.trimIndent()
    }
}
