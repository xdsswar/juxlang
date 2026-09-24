package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The library surface the bindgen pass gained from Rust's own `rust-src`
 * (ERRATA E76): `alloc` writes `impl<T> [T]` and `impl str` for primitives
 * `core` defines, so `sort`, `to_vec`, `join`, `binary_search` and
 * `str::to_uppercase` reach the `rust.std` stub. They are ordinary stub
 * members, so the editor should offer, rank and RESOLVE them like any other.
 *
 * The stub here is a slice of the generated one, written exactly as the
 * emitter writes it (annotations in front, `uint` lengths, `@MutSelf` on the
 * mutating calls), so this pins the parse of that shape as much as the
 * completion of it.
 *
 * An ARRAY has no declaration behind it, so its members come from the
 * checker's own built-in table instead; those are pinned here too, because
 * before this the popup after `values.` was simply empty.
 */
class JuxSliceAndStrMethodsTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.addFileToProject(".jux-stubs/rust/std.jux.d", STUB)
    }

    private fun names(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    fun testSliceMethodsCompleteOnAVec() {
        val o = names(
            """
            import rust.std.*;
            void main() {
                Vec<int> values = new Vec<int>();
                values.<caret>
            }
            """,
        )
        for (m in listOf(
            "sort", "sort_by", "sort_by_key", "to_vec", "repeat", "concat", "join",
            "binary_search", "contains", "reverse",
        )) {
            assertTrue("$m is offered on a Vec: $o", m in o)
        }
    }

    fun testStrMethodsCompleteOnAString() {
        val o = names(
            """
            import rust.std.*;
            void main(String s) {
                s.<caret>
            }
            """,
        )
        for (m in listOf("to_uppercase", "to_lowercase", "repeat")) {
            assertTrue("$m is offered on a String: $o", m in o)
        }
    }

    fun testASliceMethodResolvesToItsStubDeclaration() {
        myFixture.configureByText(
            "a.jux",
            """
            import rust.std.*;
            void main() {
                Vec<int> values = new Vec<int>();
                values.so<caret>rt();
            }
            """.trimIndent(),
        )
        val target = myFixture.getReferenceAtCaretPosition()?.resolve()
        assertNotNull("`sort` resolves", target)
        assertEquals("sort", (target as? dev.jux.intellij.psi.JuxNamedElement)?.name)
        assertTrue(
            "it resolves into the stub: ${target?.containingFile?.name}",
            target?.containingFile?.name?.endsWith(".jux.d") == true,
        )
    }

    fun testArrayMembersCompleteFromTheBuiltinSurface() {
        val o = names(
            """
            void main() {
                int[] values = new int[3];
                values.<caret>
            }
            """,
        )
        for (m in listOf("sort", "reverse", "contains", "indexOf", "join", "first", "last", "length")) {
            assertTrue("$m is offered on an array: $o", m in o)
        }
    }

    private companion object {
        /** A slice of the generated `rust.std` stub, as the emitter writes it. */
        val STUB = """
            package rust.std;

            @rust("std::vec::Vec")
            @RustClone
            @RustCollection
            @RustDerefs("[]")
            public class Vec<T, A> {
                public Vec();
                @MutSelf public void push(T value);
                public uint len();
                public bool is_empty();
                @MutSelf @RustBounds("T: Ord") public void sort();
                @MutSelf @RustClosureRefs("0") public void sort_by<F>((T, T) -> int compare);
                @MutSelf @RustClosureRefs("0") public void sort_by_key<K, F>((T) -> K f);
                @RustBounds("T: Clone") public Vec<T> to_vec();
                @RustBounds("T: Copy") public Vec<T> repeat(uint n);
                public Vec<T> concat<Item>();
                public String join<Separator>(Separator sep);
                @RustBounds("T: Ord") public uint binary_search(T x);
                @RustBounds("T: PartialEq") public bool contains(T x);
                @MutSelf public void reverse();
            }

            @rust("alloc::string::String")
            @RustClone
            public class String {
                public uint len();
                public bool is_empty();
                public String to_uppercase();
                public String to_lowercase();
                public String repeat(uint n);
                public String trim();
            }
        """.trimIndent()
    }
}
