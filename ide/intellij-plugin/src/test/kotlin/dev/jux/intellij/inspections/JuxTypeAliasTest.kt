package dev.jux.intellij.inspections

import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.resolve.JuxTypeEngine
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * A `type` alias and an import alias mean their target everywhere a type is
 * named (ERRATA E133, gap 37): in a supertype, a `new`, a static call, a type
 * test and a pattern. `examples/type_alias_everywhere.jux` painted
 * "cannot implement 'Sh' because it is a type alias" (E0424) over
 * `class Square implements Sh` with `type Sh = Shape;`.
 */
class JuxTypeAliasTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(
            JuxAbstractNotImplementedInspection(),
            JuxExtendsClauseInspection(),
            JuxImplementsClauseInspection(),
            JuxTypeAliasInspection(),
        )
    }

    private fun descriptions(code: String): List<String> {
        myFixture.configureByText("a.jux", code.trimIndent())
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    private fun elementAt(marker: String, type: com.intellij.psi.tree.IElementType): PsiElement {
        val offset = myFixture.file.text.indexOf(marker)
        assertTrue("marker '$marker' not found", offset >= 0)
        val leaf = myFixture.file.findElementAt(offset)!!
        return PsiTreeUtil.findFirstParent(leaf) { it.elementType === type }!!
    }

    private val shapes = """
        interface Shape {
            double area();
        }
        class Circle implements Shape {
            public double r;
            public Circle(double r) { this.r = r; }
            public double area() { return 3.0 * r * r; }
        }
        class Pair<A, B> {
            public A a;
            public B b;
            public Pair(A a, B b) { this.a = a; this.b = b; }
            public static Pair<String, int> origin() { return new Pair<String, int>("origin", 0); }
        }
        type Sh = Shape;
        type Round = Circle;
        type Named = Pair<String, int>;
        type Keyed<K> = Pair<K, int>;
        type Label = Named;
    """.trimIndent()

    fun testImplementsThroughAnAliasIsTheInterface() {
        val d = descriptions(
            """
            $shapes
            class Square implements Sh {
                public double area() { return 4.0; }
            }
            """,
        )
        assertFalse(d.toString(), d.any { it.contains("E0424") || it.contains("E0429") })
    }

    fun testTheAliasedInterfaceStillOwesItsMethods() {
        val d = descriptions(
            """
            $shapes
            class Blob implements Sh {
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.contains("E0429") && it.contains("'Shape.area'") })
    }

    fun testExtendsThroughAnAlias() {
        val d = descriptions(
            """
            class Base { }
            final class Sealed { }
            type B = Base;
            type S = Sealed;
            class Fine extends B { }
            class Wrong extends S { }
            """,
        )
        assertFalse(d.toString(), d.any { it.contains("E0423") })
        assertTrue(d.toString(), d.any { it.contains("E0420") && it.contains("'Sealed'") })
    }

    fun testNewThroughAGenericAliasHasTheTargetsType() {
        descriptions(
            """
            $shapes
            void main() {
                var kd = new Keyed<double>(1.5, 2);
                var l = new Label("a", 1);
                var c = new Round(1.0);
            }
            """,
        )
        val types = listOf("new Keyed", "new Label", "new Round").map {
            JuxTypeEngine.typeOf(elementAt(it, E.NEW_EXPRESSION)).presentable()
        }
        assertEquals(listOf("Pair<double, int>", "Pair<String, int>", "Circle"), types)
    }

    fun testStaticCallThroughAnAlias() {
        descriptions(
            """
            $shapes
            void main() {
                print(Named.origin().a);
            }
            """,
        )
        val access = elementAt("Named.origin", E.FIELD_ACCESS_EXPRESSION)
        val member = JuxTypeEngine.resolveMemberAccess(access, 0)
        assertEquals("origin", (member?.element as? JuxNamedElement)?.name)
    }

    fun testTypeTestBinderThroughAnAlias() {
        descriptions(
            """
            $shapes
            double radius(Shape s) {
                if (s => Round c) {
                    return c.r;
                }
                return 0.0;
            }
            """,
        )
        val ref = elementAt("c.r", E.REFERENCE_EXPRESSION)
        assertEquals("Circle", JuxTypeEngine.typeOf(ref).presentable())
    }

    fun testRecordPatternThroughAnAlias() {
        descriptions(
            """
            record Pt(int x, String label) { }
            type P = Pt;
            String describe(Object o) {
                return switch (o) {
                    case P(var x, var name) -> name;
                    default -> "none";
                };
            }
            """,
        )
        val ref = elementAt("name;", E.REFERENCE_EXPRESSION)
        assertEquals("String", JuxTypeEngine.typeOf(ref).presentable())
    }

    fun testAnAliasOfAnAliasReachesTheDeclaration() {
        descriptions(shapes)
        val found = JuxTypeIndex.findTypeThroughAliases(myFixture.file.firstChild, "Label")
        assertEquals("Pair", found?.name)
        // Navigation keeps the alias the user wrote.
        assertEquals(E.TYPE_ALIAS_DECLARATION, JuxTypeIndex.findType(myFixture.file.firstChild, "Label")?.node?.elementType)
    }

    fun testImportAliasMeansItsTarget() {
        myFixture.addFileToProject(
            "app/model/Shape.jux",
            """
            package app.model;
            public interface Shape { double area(); }
            """.trimIndent(),
        )
        val d = descriptions(
            """
            import app.model.Shape as S;

            class Sq implements S {
            }
            """,
        )
        assertTrue(d.toString(), d.any { it.contains("E0429") && it.contains("'Shape.area'") })
        assertEquals("Shape", JuxTypeIndex.findType(myFixture.file.firstChild, "S")?.name)
    }

    /**
     * A supertype written through an import alias is indexed under its
     * target: the subtype gutter, go-to-implementation, the hierarchy views
     * and the sealed-switch cases all read [JuxSubtypes.buildIndex].
     */
    fun testImportAliasSupertypeIsIndexedAsASubtype() {
        val shapeFile = myFixture.addFileToProject(
            "app/model/Shape.jux",
            """
            package app.model;
            public interface Shape { double area(); }
            """.trimIndent(),
        )
        myFixture.addFileToProject(
            "types.jux",
            """
            type Sh = app.model.Shape;
            class ByTypeAlias implements Sh {
                public double area() { return 2.0; }
            }
            """.trimIndent(),
        )
        descriptions(
            """
            import app.model.Shape as S;
            import app.model.{Shape as T};

            class Sq implements S {
                public double area() { return 1.0; }
            }
            class Tri implements T {
                public double area() { return 0.5; }
            }
            """,
        )
        val shape = PsiTreeUtil.findChildOfType(shapeFile, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)!!
        val index = dev.jux.intellij.resolve.JuxSubtypes.buildIndex(project)
        assertEquals(
            setOf("Sq", "Tri", "ByTypeAlias"),
            dev.jux.intellij.resolve.JuxSubtypes.directSubtypes(shape, index).mapNotNull { it.name }.toSet(),
        )
        assertEquals(
            setOf("Sq", "Tri", "ByTypeAlias"),
            dev.jux.intellij.resolve.JuxSubtypes.subtypesOf(shape).mapNotNull { it.name }.toSet(),
        )
        val sq = PsiTreeUtil.findChildrenOfType(myFixture.file, dev.jux.intellij.psi.JuxTypeDeclaration::class.java).first { it.name == "Sq" }
        assertTrue(dev.jux.intellij.resolve.JuxHierarchy.inheritsFrom(sq, "Shape"))
        // The method-level walk (override gutter, go-to-super) reaches the interface method.
        val area = dev.jux.intellij.resolve.JuxHierarchy.findSuperMethod(sq, "area", 0)
        assertEquals("app.model", area?.let { dev.jux.intellij.completion.JuxAutoImport.packageOf(PsiTreeUtil.getParentOfType(it, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)!!) })
        // Overriders of `Shape.area` found from the interface side.
        val shapeArea = PsiTreeUtil.findChildOfType(shape, dev.jux.intellij.psi.JuxMethodDeclaration::class.java)!!
        assertEquals(
            setOf("Sq", "Tri", "ByTypeAlias"),
            dev.jux.intellij.resolve.JuxSubtypes.overridingMethods(shapeArea)
                .mapNotNull { PsiTreeUtil.getParentOfType(it, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)?.name }.toSet(),
        )
    }

    fun testGroupedImportAliasMeansItsTarget() {
        myFixture.addFileToProject(
            "app/model/Shape.jux",
            """
            package app.model;
            public interface Shape { double area(); }
            """.trimIndent(),
        )
        val d = descriptions(
            """
            import app.model.{Shape as S};

            class Sq implements S {
                public double area() { return 1.0; }
            }
            """,
        )
        assertFalse(d.toString(), d.any { it.contains("E0424") || it.contains("E0429") })
        assertEquals("Shape", JuxTypeIndex.findType(myFixture.file.firstChild, "S")?.name)
    }

    // ---- E0498 / E0443 -------------------------------------------------------

    fun testAliasCycleIsReportedOnEachAlias() {
        val d = descriptions(
            """
            type A = B;
            type B = A;
            type C = int;
            """,
        )
        val cycles = d.filter { it.contains("E0498") }
        assertEquals(d.toString(), 2, cycles.size)
        assertTrue(cycles.any { it.contains("'A'") } && cycles.any { it.contains("'B'") })
    }

    fun testAliasArity() {
        val d = descriptions(
            """
            class Box<K, V> { }
            type Dict<V> = Box<String, V>;
            void main() {
                Dict<String, int> bad = new Dict<String, int>();
                Dict<int> good = new Dict<int>();
                Dict inferred = new Dict<int>();
            }
            """,
        )
        val arity = d.filter { it.contains("E0443") }
        assertEquals(d.toString(), 2, arity.size)
        assertTrue(arity.all { it.contains("Type alias 'Dict' takes 1 type argument, but 2 were supplied") })
    }
}
