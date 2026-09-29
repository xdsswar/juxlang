package dev.jux.intellij.inspections

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxAnonymousClass
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * The language changes of ERRATA E135-E143 (gaps 39-39i), as the editor sees
 * them: intersection bounds and what a bound may name (E0419, E0459), a type's
 * argument count at any depth (E0443), extending a Rust type (E0420), generic
 * methods reached through a supertype, a written `Task<T>`, anonymous classes
 * that reach the object they were built in, and field initializers that use
 * the object.
 */
class JuxGenericsAndAnonymousTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxTypeParameterBoundInspection(),
            JuxTypeArgumentCountInspection(),
            JuxTypeAliasInspection(),
            JuxExtendsClauseInspection(),
            JuxImplementsClauseInspection(),
            JuxAbstractNotImplementedInspection(),
            JuxUnresolvedReferenceInspection(),
            JuxMissingReturnInspection(),
            JuxUnusedPrivateMemberInspection(),
        )
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

    private fun resolveAt(marker: String, shift: Int = 0): PsiElement? {
        val offset = myFixture.file.text.indexOf(marker)
        assertTrue("marker '$marker' not found", offset >= 0)
        return myFixture.file.findReferenceAt(offset + shift)?.resolve()
    }

    private fun exprAt(marker: String, type: com.intellij.psi.tree.IElementType): PsiElement {
        val offset = myFixture.file.text.indexOf(marker)
        assertTrue("marker '$marker' not found", offset >= 0)
        val leaf = myFixture.file.findElementAt(offset)!!
        return PsiTreeUtil.findFirstParent(leaf) { it.node?.elementType === type }!!
    }

    private val animals = """
        interface Named { String name(); }
        interface Aged { int age(); }
        interface Scored { double score(); }
        class Animal {
            public int legs = 4;
            public String kind() { return "animal"; }
        }
        class Machine { public int gears = 3; }
        class Dog extends Animal implements Named, Aged {
            public String name() { return "rex"; }
            public int age() { return 3; }
        }
        record Point(int x, int y) {}
        enum Color { RED, GREEN }
        class Pair<K, V> {
            public K k;
            public V v;
            public Pair(K k, V v) { this.k = k; this.v = v; }
        }
    """.trimIndent()

    // ---- bounds ------------------------------------------------------------

    fun testAnIntersectionWithTheClassAfterTheInterfacesIsLegal() {
        val e = errors(
            """
            $animals
            <T extends Named & Aged & Animal> String describe(T t) { return t.name() + t.age() + t.kind(); }
            class Shelf<T extends Named & Animal> { T item; }
            """,
        )
        assertEquals(emptyList<String>(), e)
    }

    fun testTwoClassesInAnIntersectionAreE0419() {
        val e = errors(
            """
            $animals
            <T extends Animal & Named & Machine> void f(T t) {}
            <U extends Dog & Animal> void g(U u) {}
            """,
        )
        assertTrue(e.toString(), e.any { it.contains("E0419") && it.contains("no type extends both") })
        assertTrue(e.toString(), e.any { it.contains("E0419") && it.contains("`Dog` already extends `Animal`") })
    }

    fun testABoundNoOtherTypeCanMeetIsE0459() {
        val e = errors(
            """
            $animals
            <R extends Point> void a(R r) {}
            <E2 extends Color> void b(E2 c) {}
            <S extends String> void c(S s) {}
            <N extends int> void d(N n) {}
            """,
        )
        val e0459 = e.filter { it.contains("E0459") }
        assertEquals(e.toString(), 4, e0459.size)
        assertTrue(e.toString(), e0459.any { it.contains("is a record") })
        assertTrue(e.toString(), e0459.any { it.contains("is an enum") })
        assertTrue(e.toString(), e0459.any { it.contains("`String` is final") })
        assertTrue(e.toString(), e0459.any { it.contains("primitive") })
    }

    fun testAFinalClassAndAnotherParameterAreLegalBounds() {
        val e = errors(
            """
            $animals
            final class Sealed2 { }
            <T extends Sealed2> void a(T t) {}
            class Chain<K, V extends K> { }
            <K, R extends K> K up(R r) { return r; }
            """,
        )
        assertFalse(e.toString(), e.any { it.contains("E0459") || it.contains("E0419") })
    }

    fun testARustTypeBoundIsE0459AndExtendingOneIsE0420() {
        myFixture.addFileToProject(
            ".jux-stubs/rust/std.jux.d",
            "package rust.std;\n\npublic class Vec<T> {\n    public void push(T value);\n}\n",
        )
        val e = errors(
            """
            import rust.std.Vec;
            <V extends Vec<Pair<int, int>>> void f(V v) {}
            class MyVec extends Vec<int> { }
            class Pair<K, V> { }
            """,
        )
        assertTrue(e.toString(), e.any { it.contains("E0459") && it.contains("Rust type") })
        assertTrue(e.toString(), e.any { it.contains("E0420") && it.contains("Rust type") })
        assertFalse("a Rust type is not also E0423/E0429: $e", e.any { it.contains("E0423") || it.contains("E0429") })
    }

    fun testMembersOfEveryBoundResolveAndPromote() {
        myFixture.configureByText(
            "a.jux",
            """
            $animals
            <T extends Named & Aged & Scored> double total(T t) {
                var sum = t.age() + t.score();
                return sum;
            }
            """.trimIndent(),
        )
        val scoreCall = exprAt("score();\n", E.CALL_EXPRESSION)
        assertEquals("double", JuxTypeEngine.typeOf(scoreCall).presentable())
        val sum = exprAt("+ t.score()", E.BINARY_EXPRESSION)
        assertEquals("int + double promotes to double", "double", JuxTypeEngine.typeOf(sum).presentable())
        val score = resolveAt("t.score()", 2)
        assertEquals("the third bound's member navigates", "score", (score as? JuxNamedElement)?.name)
    }

    fun testIntersectionBoundMembersComplete() {
        val o = names(
            """
            $animals
            <T extends Named & Aged & Animal> void f(T t) { t.<caret> }
            """,
        )
        assertTrue(o.toString(), o.containsAll(listOf("name", "age", "kind", "legs")))
    }

    fun testAMethodTypeParameterShadowsTheClassOne() {
        myFixture.configureByText(
            "a.jux",
            """
            class Shelf<T> {
                public T item;
                public <T> T echo(T t) { return t; }
            }
            """.trimIndent(),
        )
        val target = resolveAt("T t)")
        assertTrue("the method's T: $target", target?.parent?.parent?.node?.elementType === E.METHOD_DECLARATION)
    }

    fun testAShelfOfIntDoesNotBindTheMethodsOwnT() {
        myFixture.configureByText(
            "a.jux",
            """
            class Shelf<T> {
                public T item;
                public <T> T echo(T t) { return t; }
            }
            void main() {
                Shelf<int> s = new Shelf<int>();
                var e = s.echo("s");
                var i = s.item;
            }
            """.trimIndent(),
        )
        assertFalse("int" == JuxTypeEngine.typeOf(exprAt("echo(\"s\")", E.CALL_EXPRESSION)).presentable())
        assertEquals("int", JuxTypeEngine.typeOf(exprAt("s.item;", E.FIELD_ACCESS_EXPRESSION)).presentable())
    }

    fun testNumericPromotion() {
        myFixture.configureByText(
            "a.jux",
            """
            void main() {
                int a = 1;
                long b = 2;
                double c = 3.0;
                var x = a + b;
                var y = a * c;
                var z = true ? a : c;
                var w = b + 1;
            }
            """.trimIndent(),
        )
        assertEquals("long", JuxTypeEngine.typeOf(exprAt("a + b", E.BINARY_EXPRESSION)).presentable())
        assertEquals("double", JuxTypeEngine.typeOf(exprAt("a * c", E.BINARY_EXPRESSION)).presentable())
        assertEquals("double", JuxTypeEngine.typeOf(exprAt("true ?", E.CONDITIONAL_EXPRESSION)).presentable())
        assertEquals("long", JuxTypeEngine.typeOf(exprAt("b + 1", E.BINARY_EXPRESSION)).presentable())
    }

    // ---- type argument count -------------------------------------------------

    fun testAWrongCountAtAnyDepthIsE0443() {
        val e = errors(
            """
            $animals
            class Box<T> { }
            class Holder {
                Box<Box<Pair<int>>> deep;
                Pair<int, Box<int, int>> second;
            }
            """,
        )
        val e0443 = e.filter { it.contains("E0443") }
        assertEquals(e.toString(), 2, e0443.size)
        assertTrue(e.toString(), e0443.any { it.contains("`Pair` takes 2 type arguments") && it.contains("`Pair<int>`") })
        assertTrue(e.toString(), e0443.any { it.contains("`Box` takes 1 type argument") })
    }

    fun testRightCountsAndRawUsesAreClean() {
        val e = errors(
            """
            $animals
            class Box<T> { }
            class Holder<K> {
                Box<Box<Pair<K, Box<int>>>> deep;
                Pair<(int) -> Box<int>, Box<String>[]> fns;
                <V extends Box<Pair<K, int>>> void put(V v) {}
                Box<int> b = new Box<>();
            }
            """,
        )
        assertFalse(e.toString(), e.any { it.contains("E0443") })
    }

    // ---- generic methods through a supertype, overrides, polymorphic recursion ----

    fun testTheVisitorPatternAndPolymorphicRecursionAreClean() {
        val e = errors(
            """
            interface Visitor<R> { R visitNum(int n); }
            interface Expr { <R> R accept(Visitor<R> v); }
            abstract class Tree<T> {
                abstract <R> R accept(TreeVisitor<T, R> v);
            }
            interface TreeVisitor<T, R> { R leaf(T t); }
            class Leaf extends Tree<int> {
                <R> R accept(TreeVisitor<int, R> v) { return v.leaf(1); }
            }
            class Num implements Expr {
                int value;
                Num(int value) { this.value = value; }
                public <R> R accept(Visitor<R> v) { return v.visitNum(value); }
            }
            interface Store<K> { <V extends K> int addAll(V v); }
            class Pet { }
            class PetStore implements Store<Pet> {
                public <V extends Pet> int addAll(V v) { return 1; }
            }
            class Wrap<T> { T t; Wrap(T t) { this.t = t; } }
            <T> int nest(T t, int n) { if (n == 0) { return 0; } return nest(new Wrap<T>(t), n - 1) + 1; }
            """,
        )
        assertEquals(emptyList<String>(), e)
    }

    // ---- Task<T> -------------------------------------------------------------

    fun testAWrittenTaskHasTheTaskMembers() {
        val o = names(
            """
            async int twice(int n) { return n * 2; }
            class Jobs { Task<int> pending; }
            void main() {
                Task<int> t = spawn(twice(3));
                t.<caret>
            }
            """,
        )
        assertTrue(o.toString(), o.containsAll(listOf("blockingGet", "join", "cancel", "isResolved", "map")))
    }

    fun testSpawnReturnsATask() {
        myFixture.configureByText(
            "a.jux",
            """
            async int twice(int n) { return n * 2; }
            void main() { var t = spawn(twice(3)); }
            """.trimIndent(),
        )
        val call = exprAt("spawn(", E.CALL_EXPRESSION)
        assertEquals("Task<int>", JuxTypeEngine.typeOf(call).presentable())
        val o = names("async int twice(int n) { return n * 2; }\nvoid main() { var t = spawn(twice(3)); t.<caret> }")
        assertTrue(o.toString(), "blockingGet" in o)
    }

    // ---- anonymous classes -----------------------------------------------------

    private val counter = """
        interface Listener { void on(int x); }
        abstract class Shape {
            int sides = 0;
            abstract int area();
        }
        class Counter {
            int count = 0;
            int total = 0;
            int sides = 5;
            private String log = "";
            private void note(String s) { log = log + s; }
    """.trimIndent()

    fun testAnAnonymousClassBodyIsParsedAsAClass() {
        myFixture.configureByText(
            "a.jux",
            """
            $counter
                Listener wire() {
                    return new Listener() {
                        int seen = 0;
                        public void on(int x) { count++; total += x; note("on"); seen++; }
                    };
                }
            }
            """.trimIndent(),
        )
        val anon = PsiTreeUtil.findChildOfType(myFixture.file, JuxAnonymousClass::class.java)
        assertNotNull("an anonymous class node", anon)
        assertNull(anon!!.name)
        assertEquals("Listener", anon.supertypeReference()?.text)
        // Its members are its own, not the enclosing class's.
        val counterMembers = JuxTypeEngine.membersOf(
            JuxTypeEngine.selfType(PsiTreeUtil.findChildrenOfType(myFixture.file, dev.jux.intellij.psi.JuxTypeDeclaration::class.java).first { it.name == "Counter" }),
        ).mapNotNull { (it.element as? JuxNamedElement)?.name }
        assertFalse("`seen` belongs to the anonymous class: $counterMembers", "seen" in counterMembers)
    }

    fun testBareOuterMembersResolveInsideAnAnonymousClass() {
        myFixture.configureByText(
            "a.jux",
            """
            $counter
                Listener wire() {
                    return new Listener() {
                        public void on(int x) { count++; total += x; note("on"); }
                    };
                }
            }
            """.trimIndent(),
        )
        val count = resolveAt("count++")
        assertEquals("count", (count as? JuxNamedElement)?.name)
        assertEquals("Counter", PsiTreeUtil.getParentOfType(count, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)?.name)
        assertEquals("note", (resolveAt("note(\"on\")") as? JuxNamedElement)?.name)
        assertEquals("x", (resolveAt("x; note") as? JuxNamedElement)?.name)
        assertEquals(emptyList<String>(), errors(myFixture.file.text))
    }

    fun testShadowingOwnAndSupertypeMembersThenLocalsThenTheOuterObject() {
        myFixture.configureByText(
            "a.jux",
            """
            $counter
                Shape make() {
                    int total = 100;
                    return new Shape() {
                        public int area() { return sides * total + count; }
                    };
                }
            }
            """.trimIndent(),
        )
        // `sides`: the superclass's field wins over the enclosing object's.
        val sides = resolveAt("sides * total")
        assertEquals("Shape", PsiTreeUtil.getParentOfType(sides, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)?.name)
        // `total`: the captured local wins over the enclosing object's field.
        val total = resolveAt("total + count")
        assertEquals(E.LOCAL_VARIABLE, total?.node?.elementType)
        // `count`: only the enclosing object has it.
        val count = resolveAt("count;")
        assertEquals("Counter", PsiTreeUtil.getParentOfType(count, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)?.name)
    }

    fun testThisInsideAnAnonymousClassIsTheAnonymousObject() {
        myFixture.configureByText(
            "a.jux",
            """
            $counter
                Shape make() {
                    return new Shape() {
                        public int area() { return this.sides; }
                    };
                }
            }
            """.trimIndent(),
        )
        val self = exprAt("this.sides", E.THIS_EXPRESSION)
        val t = JuxTypeEngine.typeOf(self) as JuxType.ClassType
        assertTrue("`this` is the anonymous object", t.decl is JuxAnonymousClass)
        val sides = resolveAt("sides; }", 0)
        assertEquals("Shape", PsiTreeUtil.getParentOfType(sides, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)?.name)
    }

    fun testCompletionInsideAnAnonymousClassOffersOuterMembersPrivatesIncluded() {
        val o = names(
            """
            $counter
                Listener wire() {
                    return new Listener() {
                        public void on(int x) { <caret> }
                    };
                }
            }
            """,
        )
        assertTrue(o.toString(), o.containsAll(listOf("count", "total", "note", "log", "on", "x")))
    }

    fun testCompletionAfterThisInsideAnAnonymousClassIsTheAnonymousObjects() {
        val o = names(
            """
            $counter
                Shape make() {
                    return new Shape() {
                        int own = 1;
                        public int area() { return this.<caret> }
                    };
                }
            }
            """,
        )
        assertTrue(o.toString(), o.containsAll(listOf("own", "sides", "area")))
        assertFalse("the enclosing object's members are not on `this`: $o", "count" in o)
    }

    // ---- field initializers ----------------------------------------------------

    fun testAFieldInitializerMayUseTheObject() {
        myFixture.configureByText(
            "a.jux",
            """
            class Worker { Worker(Mill m) {} }
            class Mill {
                int a = 1;
                int b = a + 1;
                int y = twice(3);
                Worker w = new Worker(this);
                int twice(int n) { return n * 2; }
            }
            """.trimIndent(),
        )
        assertEquals(emptyList<String>(), errors(myFixture.file.text))
        assertEquals("twice", (resolveAt("twice(3)") as? JuxNamedElement)?.name)
        val self = exprAt("this)", E.THIS_EXPRESSION)
        assertEquals("Mill", JuxTypeEngine.typeOf(self).presentable())
    }

    // ---- the mirrored codes are listed for LSP dedup -------------------------------

    fun testTheNewCodesAreMirrored() {
        for (code in listOf("E0419", "E0459", "E0443", "E0420")) {
            assertTrue(code, code in JuxMirroredDiagnostics.MIRRORED)
        }
    }
}
