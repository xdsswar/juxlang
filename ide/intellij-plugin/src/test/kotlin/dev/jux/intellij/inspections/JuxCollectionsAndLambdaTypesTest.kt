package dev.jux.intellij.inspections

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * The language changes of ERRATA E144-E145 (gaps 40, 40b), and the items
 * 0.1.9 left open, as the editor sees them: a lambda's own function type, so
 * `var gv = () -> v; gv().push(1)` types and completes; `int[]? xs;` as a
 * local; E0453 for a `var` lambda nothing gives a parameter type to; E0702 for
 * a write to a worker's copy of a collection; E0429 on an anonymous class;
 * E0459 on an array, nullable or pointer bound; and `Task.await()` /
 * `Task.delay(..)`.
 */
class JuxCollectionsAndLambdaTypesTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxAbstractNotImplementedInspection(),
            JuxTypeParameterBoundInspection(),
            JuxUninferableLambdaInspection(),
            JuxRefBindingInspection(),
            JuxUnresolvedReferenceInspection(),
        )
    }

    private fun addStub() {
        myFixture.addFileToProject(".jux-stubs/rust/std.jux.d", STUB)
    }

    private fun errors(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting()
            .filter { it.severity === HighlightSeverity.ERROR }
            .mapNotNull { it.description }
    }

    private fun names(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.completeBasic()?.map { it.lookupString } ?: emptyList()
    }

    private fun exprAt(marker: String, type: com.intellij.psi.tree.IElementType): PsiElement {
        val offset = myFixture.file.text.indexOf(marker)
        assertTrue("marker '$marker' not found", offset >= 0)
        val leaf = myFixture.file.findElementAt(offset)!!
        return PsiTreeUtil.findFirstParent(leaf) { it.node?.elementType === type }!!
    }

    // ---- a lambda's own function type (E145) ----------------------------------

    fun testAVarLambdaHasAFunctionTypeAndItsCallTypes() {
        addStub()
        myFixture.configureByText(
            "a.jux",
            """
            import rust.std.*;
            void main() {
                var v = new Vec<int>();
                var gv = () -> v;
                gv().push(1);
            }
            """.trimIndent(),
        )
        val lambda = exprAt("() -> v", E.LAMBDA_EXPRESSION)
        assertEquals("() -> Vec<int>", JuxTypeEngine.typeOf(lambda).presentable())
        val call = exprAt("gv()", E.CALL_EXPRESSION)
        assertEquals("Vec<int>", JuxTypeEngine.typeOf(call).presentable())
        val push = myFixture.file.findReferenceAt(myFixture.file.text.indexOf("push(1)"))?.resolve()
        assertEquals("push", (push as? JuxNamedElement)?.name)
    }

    fun testAVarLambdaCallCompletesTheResultsMembers() {
        addStub()
        val o = names(
            """
            import rust.std.*;
            void main() {
                var v = new Vec<int>();
                var gv = () -> v;
                gv().<caret>
            }
            """,
        )
        assertTrue(o.toString(), "push" in o && "len" in o)
    }

    fun testALambdaTypeFromItsParametersAndABlockBody() {
        myFixture.configureByText(
            "a.jux",
            """
            void main() {
                var add = (int a, int b) -> { if (a > b) { return a; } return b; };
                var say = (String s) -> { print(s); };
                var r = add(1, 2);
            }
            """.trimIndent(),
        )
        assertEquals("(int, int) -> int", JuxTypeEngine.typeOf(exprAt("(int a", E.LAMBDA_EXPRESSION)).presentable())
        assertEquals("(String) -> void", JuxTypeEngine.typeOf(exprAt("(String s", E.LAMBDA_EXPRESSION)).presentable())
        assertEquals("int", JuxTypeEngine.typeOf(exprAt("add(1, 2)", E.CALL_EXPRESSION)).presentable())
    }

    fun testANestedReturnIsNotTheLambdasOwn() {
        myFixture.configureByText(
            "a.jux",
            """
            void main() {
                var f = () -> { var g = () -> { return "s"; }; return 1; };
            }
            """.trimIndent(),
        )
        val outer = exprAt("() -> { var g", E.LAMBDA_EXPRESSION)
        assertEquals("() -> int", JuxTypeEngine.typeOf(outer).presentable())
    }

    fun testAWorkerSpawnOfABlockLambdaIsATaskOfItsReturn() {
        myFixture.configureByText(
            "a.jux",
            "async void main() { var t = Worker.spawn(() -> { return 7; }); }",
        )
        assertEquals("Task<int>", JuxTypeEngine.typeOf(exprAt("Worker.spawn", E.CALL_EXPRESSION)).presentable())
    }

    // ---- `int[]? xs;` is a local (E145) -----------------------------------------

    fun testANullableArrayLocalParses() {
        myFixture.configureByText(
            "a.jux",
            """
            void main() {
                int[]? xs;
                int[]? ys = null;
                xs = new int[2];
            }
            """.trimIndent(),
        )
        assertNull(PsiTreeUtil.findChildOfType(myFixture.file, PsiErrorElement::class.java))
        val locals = PsiTreeUtil.findChildrenOfType(myFixture.file, JuxNamedElement::class.java)
            .filter { it.node.elementType === E.LOCAL_VARIABLE }.mapNotNull { it.name }
        assertEquals(listOf("xs", "ys"), locals)
        val xs = PsiTreeUtil.findChildrenOfType(myFixture.file, JuxNamedElement::class.java).first { it.name == "xs" }
        assertEquals("int[]?", JuxTypeEngine.declaredType(xs).presentable())
    }

    // ---- E0453: a `var` lambda nothing gives a parameter type to (E145) --------

    fun testAnUnusedUntypedVarLambdaIsE0453() {
        val e = errors(
            """
            public int apply((int) -> int f, int x) { return f(x); }
            public void main() {
                var unused = (x) -> x + 1;
                var called = (x) -> x * 2;
                var passed = (x) -> x - 1;
                var typed = (int x) -> x;
                var bare = y -> y;
                print(called(4));
                print(apply(passed, 10));
            }
            """,
        )
        val e0453 = e.filter { it.contains("E0453") }
        assertEquals(e.toString(), 2, e0453.size)
        assertTrue(
            e.toString(),
            e0453.any {
                it == "cannot infer the type of `x`, a parameter of the lambda `unused`: the lambda is never " +
                    "called or passed anywhere that would give it one (E0453)"
            },
        )
        assertTrue(e.toString(), e0453.any { it.contains("`y`, a parameter of the lambda `bare`") })
    }

    // ---- E0702: a write to a worker's copy (E144) ------------------------------

    fun testAWriteToAWorkersCopyIsE0702() {
        addStub()
        val e = errors(
            """
            import rust.std.*;
            public async void main() {
                var xs = new Vec<int>();
                xs.push(1);
                int[] arr = new int[3];
                var reads = Worker.spawn(() -> (int) xs.len() + arr[0]);
                var pushes = Worker.spawn(() -> { xs.push(2); return 0; });
                var stores = Worker.spawn(() -> { arr[1] = 5; return 0; });
                var copies = Worker.spawn(() -> {
                    var mine = xs.clone();
                    mine.push(3);
                    return (int) mine.len();
                });
                print(await reads);
                print(await pushes);
                print(await stores);
                print(await copies);
            }
            """,
        )
        val e0702 = e.filter { it.contains("E0702") }
        assertEquals(e.toString(), 2, e0702.size)
        assertTrue(
            e.toString(),
            e0702.contains(
                "`xs` is written inside a `Worker.spawn` closure, but a worker runs on another thread with its " +
                    "OWN copy of a captured collection (§18.2), so the write would never reach the `xs` outside it (E0702)",
            ),
        )
        assertTrue(e.toString(), e0702.any { it.startsWith("`arr` is written") && it.contains("captured array") })
    }

    fun testAWriteOutsideAWorkerIsFine() {
        addStub()
        val e = errors(
            """
            import rust.std.*;
            public async void main() {
                var xs = new Vec<int>();
                var f = () -> { xs.push(2); };
                f();
                var t = spawn(fill(xs));
                await t;
            }
            async int fill(Vec<int> v) { v.push(1); return 0; }
            """,
        )
        assertTrue(e.toString(), e.none { it.contains("E0702") })
    }

    // ---- E0429 on an anonymous class (0.1.9 open item) ---------------------------

    private val shapes = """
        interface Shape { int area(); String name(); }
        abstract class Base { abstract int weight(); }
    """.trimIndent()

    fun testAnAnonymousClassMissingAMethodIsE0429() {
        val e = errors(
            """
            $shapes
            void main() {
                var s = new Shape() {
                    public int area() { return 1; }
                };
                var b = new Base() { };
                var ok = new Shape() {
                    public int area() { return 1; }
                    public String name() { return "ok"; }
                };
            }
            """,
        )
        val e0429 = e.filter { it.contains("E0429") }
        assertEquals(e.toString(), 2, e0429.size)
        assertTrue(e.toString(), "Class 'Shape\$anon' doesn't implement abstract method(s): 'Shape.name' (E0429)" in e0429)
        assertTrue(e.toString(), "Class 'Base\$anon' doesn't implement abstract method(s): 'Base.weight' (E0429)" in e0429)
    }

    fun testImplementMethodsFixesAnAnonymousClass() {
        myFixture.configureByText(
            "a.jux",
            """
            $shapes
            void main() {
                var s = new Sh<caret>ape() {
                    public int area() { return 1; }
                };
            }
            """.trimIndent(),
        )
        val fix = myFixture.getAllQuickFixes().firstOrNull { it.text == "Implement methods" }
        assertNotNull("an Implement methods fix", fix)
        assertNull("no Make abstract fix for an anonymous class", myFixture.getAllQuickFixes().firstOrNull { it.text.contains("abstract") })
        myFixture.launchAction(fix!!)
        assertTrue(myFixture.file.text, myFixture.file.text.contains("name()"))
        val after = myFixture.doHighlighting().filter { it.severity === HighlightSeverity.ERROR }.mapNotNull { it.description }
        assertTrue(after.toString(), after.none { it.contains("E0429") })
    }

    // ---- E0459 on an array, nullable or pointer bound (0.1.9 open item) ---------

    fun testArrayNullableAndPointerBoundsAreE0459() {
        val e = errors(
            """
            interface Named { String name(); }
            class P1<T extends int*> { }
            class P2<T extends Named*> { }
            class P3<T extends Named[]> { }
            class P4<T extends Named?> { }
            class P5<T extends Named & int[]> { }
            class P6<T extends int[][]> { }
            class P7<T extends Unknown[]> { }
            """,
        )
        val e0459 = e.filter { it.contains("E0459") }
        assertEquals(e.toString(), 5, e0459.size)
        assertTrue(e.toString(), "`T extends int` admits only `int` itself: `int` is a primitive type (§T.4.6) (E0459)" in e0459)
        assertTrue(e.toString(), "`T extends Named[]` admits only `Named[]` itself: `Named[]` is an array type (§T.4.6) (E0459)" in e0459)
        assertTrue(e.toString(), "`T extends Named?` admits only `Named?` itself: `Named?` is a nullable type (§T.4.6) (E0459)" in e0459)
        assertTrue(e.toString(), e0459.any { it.startsWith("`T extends int[]` admits") })
        assertTrue(e.toString(), e0459.any { it.startsWith("`T extends int[][]` admits") })
    }

    fun testAGenericBoundKeepsItsArgumentsInTheMessage() {
        addStub()
        val e = errors(
            """
            import rust.std.*;
            class Pair<K, V> { }
            <V extends Vec<Pair<int, int>>> void f(V v) {}
            <W extends Vec<int>> void g(W w) {}
            """,
        )
        assertTrue(e.toString(), e.any { it.startsWith("`V extends Vec<Pair<int, int>>` admits only `Vec<Pair<int, int>>` itself") })
        assertTrue(e.toString(), e.any { it.startsWith("`W extends Vec<int>` admits only `Vec<int>` itself") })
    }

    // ---- E0443 stays silent on a name another file's type may mean -------------

    fun testAStrangersSameNamedTypeIsNotHeldToItsCount() {
        myFixture.enableInspections(JuxTypeArgumentCountInspection())
        myFixture.addFileToProject("plain.jux", "class Box { }\n")
        myFixture.addFileToProject("generic.jux", "class Box<T> { T t; }\n")
        val e = errors(
            """
            import rust.std.*;
            interface Boxes<B> { int weigh(B b); }
            class Leaf implements Boxes<Box<Leaf>> {
                public int weigh(Box<Leaf> b) { return 5; }
            }
            """,
        )
        assertTrue(e.toString(), e.none { it.contains("E0443") })
    }

    // ---- Task.await() and Task.delay (0.1.9 open item) ---------------------------

    fun testTaskAwaitAndDelayComplete() {
        val members = names(
            """
            async int twice(int n) { return n * 2; }
            async void main() {
                var t = spawn(twice(3));
                t.<caret>
            }
            """,
        )
        assertTrue(members.toString(), "await" in members)
        val statics = names("async void main() { Task.<caret> }")
        assertTrue(statics.toString(), "delay" in statics)
    }

    fun testTaskAwaitAndDelayType() {
        myFixture.configureByText(
            "a.jux",
            """
            async int twice(int n) { return n * 2; }
            async void main() {
                var t = spawn(twice(3));
                var v = t.await();
                var d = Task.delay(200);
            }
            """.trimIndent(),
        )
        assertEquals("int", JuxTypeEngine.typeOf(exprAt("t.await()", E.CALL_EXPRESSION)).presentable())
        assertEquals("Task<void>", JuxTypeEngine.typeOf(exprAt("Task.delay", E.CALL_EXPRESSION)).presentable())
        val e = myFixture.doHighlighting().filter { it.severity === HighlightSeverity.ERROR }.mapNotNull { it.description }
        assertEquals(emptyList<String>(), e)
    }

    // ---- the mirrored codes are listed for LSP dedup -------------------------------

    fun testTheNewCodesAreMirroredAndE0438IsGone() {
        for (code in listOf("E0453", "E0702", "E0429", "E0459")) {
            assertTrue(code, code in JuxMirroredDiagnostics.MIRRORED)
        }
        assertFalse("E0438 is retired (ERRATA E145)", "E0438" in JuxMirroredDiagnostics.MIRRORED)
    }

    fun testTheLambdaTypeIsAFunctionType() {
        myFixture.configureByText("a.jux", "void main() { var f = (int x) -> x; }")
        assertTrue(JuxTypeEngine.typeOf(exprAt("(int x)", E.LAMBDA_EXPRESSION)) is JuxType.FunctionType)
    }

    private companion object {
        /** A slice of the generated `rust.std` stub, written as the emitter writes it. */
        val STUB = """
            package rust.std;

            @rust("std::vec::Vec")
            @RustClone
            @RustCollection
            public class Vec<T, A> {
                public Vec();
                @MutSelf public void push(T value);
                public uint len();
                public Vec<T> clone();
            }
        """.trimIndent()
    }
}
