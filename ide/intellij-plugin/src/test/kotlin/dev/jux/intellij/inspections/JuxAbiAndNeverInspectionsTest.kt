package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * The editor mirrors of the Phase 8 ABI rules and the Phase 4 library rules:
 * `never` functions (E0485/E0486) and how a call to one ends a path, W0820,
 * the layout/ABI checks (E0519-E0522, E0950, E0951), E0487 time spans, and
 * the compiler-intrinsic names (`alignof`, `never`, the range types) that
 * must resolve.
 */
class JuxAbiAndNeverInspectionsTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxNeverFunctionInspection(),
            JuxMissingReturnInspection(),
            JuxUnreachableCodeInspection(),
            JuxSafetyCommentInspection(),
            JuxAbiRulesInspection(),
            JuxTimeSpanInspection(),
            JuxUnresolvedReferenceInspection(),
        )
    }

    private fun descriptions(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    private fun applyFix(code: String, fix: String): String {
        myFixture.configureByText("a.jux", code.trimIndent())
        myFixture.doHighlighting()
        val action = myFixture.getAllQuickFixes().firstOrNull { it.text == fix }
            ?: error("no fix '$fix' among ${myFixture.getAllQuickFixes().map { it.text }}")
        myFixture.launchAction(action)
        return myFixture.editor.document.text
    }

    // ---- never ------------------------------------------------------------

    fun testNeverFunctionRules() {
        val d = descriptions(
            """
            never stop(int n) {
                if (n > 0) {
                    throw new IllegalStateException("stop");
                }
            }

            never quit() {
                return;
            }

            never loop() {
                () -> int f = () -> {
                    return 1;
                };
                while (true) {
                    print(f());
                }
            }

            never fine() {
                throw new IllegalStateException("x");
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.contains("`stop` returns `never` but can reach the end") })
        assertTrue(d.toString(), d.any { it.contains("`quit` returns `never`, so it cannot `return`") })
        assertFalse(d.toString(), d.any { it.contains("`loop`") })
        assertFalse(d.toString(), d.any { it.contains("`fine`") })
        assertFalse(d.toString(), "Missing return statement" in d)
    }

    fun testCallToNeverEndsThePath() {
        val d = descriptions(
            """
            never fail(String what) {
                throw new IllegalStateException(what);
            }

            never failTwice(String what) {
                fail(what);
            }

            int port(String value) {
                if (value == "80") {
                    return 80;
                }
                fail("bad port");
            }

            int after(String value) {
                fail("no");
                print("never printed");
            }
            """,
        )
        assertFalse(d.toString(), "Missing return statement" in d)
        assertFalse(d.toString(), d.any { it.contains("failTwice") })
        assertTrue(d.toString(), "Unreachable code" in d)
    }

    fun testNeverFixAddsAThrow() {
        val out = applyFix(
            """
            never stop(int n) {
                print(n);
            }
            """,
            "Add 'throw' at the end",
        )
        assertTrue(out, out.contains("throw new IllegalStateException(\"unreachable\");"))
    }

    // ---- W0820 ------------------------------------------------------------

    fun testSafetyCommentPlacements() {
        val d = descriptions(
            """
            void a() {
                unsafe {
                    print(1);
                }
            }

            void b() {
                // SAFETY: nothing is dereferenced.
                unsafe {
                    print(2);
                }
            }

            void c() {
                unsafe { // SAFETY: same line.
                    print(3);
                }
            }

            void d() {
                // SAFETY: too far away.

                unsafe {
                    print(4);
                }
            }

            unsafe void e() {
                print(5);
            }
            """,
        )
        assertEquals(d.toString(), 2, d.count { it.contains("SAFETY:") && it.contains("W0820") })
    }

    fun testSafetyFixWritesTheComment() {
        val out = applyFix(
            """
            void a() {
                unsafe {
                    print(1);
                }
            }
            """,
            "Add '// SAFETY:' comment",
        )
        assertTrue(out, out.contains("    // SAFETY: \n    unsafe {"))
    }

    // ---- layout / ABI -------------------------------------------------------

    fun testAlignRules() {
        val d = descriptions(
            """
            @align(48)
            struct NotPowerOfTwo {
                long v;
            }

            @align(size)
            struct NotALiteral {
                long v;
            }

            struct FieldAligned {
                @align(64)
                long v;
            }

            @align(8)
            enum Mode {
                On,
                Off
            }

            @align(64)
            struct Fine {
                long v;
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.contains("48 is not a power of two (E0519)") })
        assertTrue(d.toString(), d.any { it.contains("must be an integer literal") })
        assertTrue(d.toString(), d.any { it.contains("cannot be put on a field (E0520)") })
        assertTrue(d.toString(), d.any { it.contains("cannot be put on an enum (E0520)") })
        assertEquals(d.toString(), 4, d.count { it.contains("E0519") || it.contains("E0520") })
    }

    fun testArrayToPointerRules() {
        val d = descriptions(
            """
            public void main() {
                int[] nums = {1, 2, 3};
                int[][] grid = {{1, 2}, {3, 4}};
                // SAFETY: test.
                unsafe {
                    int* fine = nums as int*;
                    int* temporary = (new int[3]) as int*;
                    int* nested = grid as int*;
                    int** twice = nums as int**;
                }
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.contains("a temporary array") })
        assertTrue(d.toString(), d.any { it.contains("only a one-dimensional array") })
        assertTrue(d.toString(), d.any { it.contains("not to a pointer to a pointer") })
        assertEquals(d.toString(), 3, d.count { it.contains("E0521") })
    }

    fun testTransmuteRules() {
        val d = descriptions(
            """
            public void main() {
                float f = 1.5f;
                // SAFETY: test.
                unsafe {
                    long a = transmute<i32, long>(7);
                    long b = transmute<int, long>(7);
                    u32 c = transmute<float>(f);
                    uint fine = transmute<int, uint>(-1);
                    u32 bits = transmute<float, u32>(f);
                }
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.contains("`i32` is 4 bytes and `long` is 8 bytes") })
        assertTrue(d.toString(), d.any { it.contains("`int` is one machine word") })
        assertTrue(d.toString(), d.any { it.contains("exactly two type arguments") })
        assertEquals(d.toString(), 3, d.count { it.contains("E0522") })
    }

    fun testOperatorCoherence() {
        val d = descriptions(
            """
            class Vec3 {
                public double x;
                public Vec3(double x) { this.x = x; }
            }

            public Vec3 operator*(double k, Vec3 v) { return new Vec3(v.x * k); }
            public Vec3 operator*(double k, Vec3 v) { return new Vec3(v.x * k * 2.0); }

            public int operator+(double a, int b) { return (int) a + b; }
            """,
        )
        assertEquals(d.toString(), 1, d.count { it.contains("E0951") })
        assertEquals(d.toString(), 1, d.count { it.contains("E0950") })
    }

    // ---- E0487 ------------------------------------------------------------

    fun testTimeSpanArgument() {
        val d = descriptions(
            """
            async int work() {
                return 1;
            }

            async void run() {
                await Task.delay("soon");
                print(await withTimeout(2.5, async () -> await work()));
                await Task.delay(10);
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.contains("found String (E0487)") })
        assertTrue(d.toString(), d.any { it.contains("found double (E0487)") })
        assertEquals(d.toString(), 2, d.count { it.contains("E0487") })
    }

    // ---- intrinsic names ------------------------------------------------------

    fun testIntrinsicNamesResolve() {
        val d = descriptions(
            """
            never fail() {
                throw new IllegalStateException("x");
            }

            int total(ExclusiveRange<int> r) {
                int sum = 0;
                for (var i : r) {
                    sum += i;
                }
                return sum;
            }

            InclusiveRange<char> letters() {
                return 'a'..='e';
            }

            String listed(SteppedRange<int> r) {
                return "";
            }

            public void main() {
                print(alignof(long));
                var evens = 0..10 step 2;
                print(evens.step);
            }
            """,
        )
        assertFalse(d.toString(), d.any { it.startsWith("Cannot resolve") })
    }
}
