package dev.jux.intellij.resolve

import com.intellij.ide.hierarchy.HierarchyNodeDescriptor
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.hierarchy.Direction
import dev.jux.intellij.resolve.hierarchy.JuxHierarchyNodeDescriptor
import dev.jux.intellij.resolve.hierarchy.JuxTypeHierarchyTreeStructure

/**
 * Type Hierarchy (`Ctrl+H`) — the three tree structures.
 *
 * Asserted on the structure rather than on the Swing browser, because the tree
 * content is the part that can be wrong; the panel around it is the platform's.
 */
class JuxTypeHierarchyTest : BasePlatformTestCase() {

    private val corpus = """
        interface Drawable { void draw(); }
        class Shape implements Drawable {
            public void draw() { }
        }
        class Circle extends Shape { }
        class Square extends Shape { }
    """.trimIndent()

    fun testSubtypesWalkDown() {
        val children = childrenOf("Shape", Direction.SUBTYPES)
        assertSameElements(children, listOf("Circle", "Square"))
    }

    fun testSupertypesWalkUp() {
        val children = childrenOf("Circle", Direction.SUPERTYPES)
        assertSameElements(children, listOf("Shape"))
    }

    fun testAnInterfaceFindsItsImplementors() {
        val children = childrenOf("Drawable", Direction.SUBTYPES)
        assertSameElements(children, listOf("Shape"))
    }

    fun testTheCombinedViewIsRootedAtTheTopmostAncestor() {
        // Java's "Type Hierarchy" view shows the base type in context, so
        // `Circle` must appear under `Shape`, not as a root of its own.
        val structure = structureFor("Circle", Direction.BOTH)
        val root = structure.rootElement as HierarchyNodeDescriptor
        assertEquals("Shape", (root.psiElement as JuxTypeDeclaration).name)
    }

    fun testACycleDoesNotHangTheCombinedView() {
        // `class A extends B` / `class B extends A` is perfectly typable, and
        // the root walk must terminate on it rather than spin.
        myFixture.configureByText("a.jux", "class A extends B { }\nclass B extends A { }")
        val a = typeNamed("A")
        val structure = JuxTypeHierarchyTreeStructure(a, Direction.BOTH)
        assertNotNull(structure.rootElement)
    }

    fun testTheNodeLabelNamesTheKind() {
        myFixture.configureByText("a.jux", corpus)
        val descriptor = JuxHierarchyNodeDescriptor(project, null, typeNamed("Drawable"), isBase = true)
        descriptor.update()
        assertEquals("interface Drawable", descriptor.toString())
    }

    // ---- helpers -----------------------------------------------------------

    private fun childrenOf(typeName: String, direction: Direction): List<String> {
        val structure = structureFor(typeName, direction)
        val root = structure.rootElement as HierarchyNodeDescriptor
        return structure.getChildElements(root)
            .filterIsInstance<HierarchyNodeDescriptor>()
            .mapNotNull { (it.psiElement as? JuxTypeDeclaration)?.name }
    }

    private fun structureFor(typeName: String, direction: Direction): JuxTypeHierarchyTreeStructure {
        myFixture.configureByText("a.jux", corpus)
        return JuxTypeHierarchyTreeStructure(typeNamed(typeName), direction)
    }

    private fun typeNamed(name: String): JuxTypeDeclaration =
        myFixture.file.children
            .filterIsInstance<JuxTypeDeclaration>()
            .first { it.name == name }
}
