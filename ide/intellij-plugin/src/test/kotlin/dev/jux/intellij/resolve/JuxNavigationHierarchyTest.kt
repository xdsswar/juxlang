package dev.jux.intellij.resolve

import com.intellij.ide.hierarchy.HierarchyBrowserManager
import com.intellij.ide.hierarchy.HierarchyNodeDescriptor
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.codeInsight.JuxGotoSuperHandler
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.hierarchy.JuxCallHierarchy
import dev.jux.intellij.resolve.hierarchy.JuxCalleeTreeStructure
import dev.jux.intellij.resolve.hierarchy.JuxCallerTreeStructure
import dev.jux.intellij.resolve.hierarchy.JuxMethodHierarchyTreeStructure
import dev.jux.intellij.resolve.hierarchy.JuxMethodNodeDescriptor
import dev.jux.intellij.resolve.hierarchy.JuxMethodState

/**
 * Go to Super (Ctrl+U), Call Hierarchy (Ctrl+Alt+H) and Method Hierarchy
 * (Ctrl+Shift+H). The trees are asserted on their structures, as the type
 * hierarchy test does: the content is what can be wrong.
 */
class JuxNavigationHierarchyTest : BasePlatformTestCase() {

    private val shapes = """
        interface Drawable { void draw(); }
        abstract class Shape implements Drawable {
            public abstract double area();
            public void draw() { print("shape"); }
        }
        class Circle extends Shape {
            public double area() { return 3.0; }
            public void draw() { print("circle"); }
        }
        class Square extends Shape {
            public double area() { return 4.0; }
        }
        class Blob extends Shape { }
    """.trimIndent()

    // ---- Go to Super --------------------------------------------------------

    fun testGotoSuperFromAMethodJumpsToTheOverriddenOne() {
        myFixture.configureByText("a.jux", shapes.replace("print(\"circle\")", "print(\"circ<caret>le\")"))
        myFixture.performEditorAction("GotoSuperMethod")
        val at = myFixture.file.findElementAt(myFixture.caretOffset)
        val method = PsiTreeUtil.getParentOfType(at, JuxMethodDeclaration::class.java, false)
        assertEquals("draw", method?.name)
        assertEquals("Shape", PsiTreeUtil.getParentOfType(method, JuxTypeDeclaration::class.java)?.name)
    }

    fun testGotoSuperFromAnImplementationReachesTheInterface() {
        myFixture.configureByText("a.jux", shapes)
        val targets = JuxGotoSuperHandler.superMethods(method("Shape", "draw"))
        assertEquals(listOf("Drawable"), targets.map { JuxHierarchy.enclosingType(it)?.name })
    }

    fun testGotoSuperOutsideAMethodListsTheSupertypes() {
        myFixture.configureByText("a.jux", shapes.replace("class Circle extends", "class Cir<caret>cle extends"))
        val targets = JuxGotoSuperHandler.targets(myFixture.file.findElementAt(myFixture.caretOffset)!!)
        assertEquals(listOf("Shape"), targets.map { (it as JuxNamedElement).name })
    }

    // ---- Call Hierarchy -----------------------------------------------------

    private val calls = """
        class Util {
            public static int helper(int x) { return x + 1; }
        }
        class App {
            public int first() { return Util.helper(1); }
            public int second() { return Util.helper(2) + Util.helper(3); }
            public int third() { return first() + second(); }
        }
    """.trimIndent()

    fun testCallersAreTheMethodsThatCallIt() {
        myFixture.configureByText("a.jux", calls)
        val structure = JuxCallerTreeStructure(project, method("Util", "helper"))
        val root = structure.rootElement as HierarchyNodeDescriptor
        val children = structure.getChildElements(root).filterIsInstance<HierarchyNodeDescriptor>()
        assertSameElements(children.map { (it.psiElement as JuxMethodDeclaration).name }, listOf("first", "second"))
        // `second` calls it twice, and says so.
        val second = children.first { (it.psiElement as JuxMethodDeclaration).name == "second" }
        second.update()
        assertTrue(second.toString(), second.toString().contains("2 usages"))
    }

    fun testCalleesAreTheMethodsItCalls() {
        myFixture.configureByText("a.jux", calls)
        val structure = JuxCalleeTreeStructure(project, method("App", "third"))
        val root = structure.rootElement as HierarchyNodeDescriptor
        val children = structure.getChildElements(root).filterIsInstance<HierarchyNodeDescriptor>()
        assertEquals(listOf("first", "second"), children.map { (it.psiElement as JuxMethodDeclaration).name })
    }

    fun testACallThroughTheBaseTypeCountsAsACaller() {
        myFixture.configureByText(
            "a.jux",
            shapes + "\nvoid paint(Shape s) { s.draw(); }\n",
        )
        val callers = JuxCallHierarchy.callers(method("Circle", "draw")).keys
        assertTrue(callers.toString(), callers.any { (it as? JuxMethodDeclaration)?.name == "paint" })
    }

    fun testTheCallerLabelNamesTheTypeAndParameters() {
        myFixture.configureByText("a.jux", calls)
        assertEquals("Util.helper(int)", JuxCallHierarchy.label(method("Util", "helper")))
    }

    // ---- Method Hierarchy ---------------------------------------------------

    fun testTheMethodHierarchyShowsWhoDefinesWhat() {
        myFixture.configureByText("a.jux", shapes)
        val structure = JuxMethodHierarchyTreeStructure(project, method("Circle", "area"))
        val root = structure.rootElement as JuxMethodNodeDescriptor
        // Rooted at the topmost declaration: the abstract one in Shape.
        assertEquals("Shape", root.type.name)
        val states = structure.getChildElements(root).filterIsInstance<JuxMethodNodeDescriptor>()
            .associate { it.type.name to it.state }
        assertEquals(JuxMethodState.DEFINED, states["Circle"])
        assertEquals(JuxMethodState.DEFINED, states["Square"])
        // Blob is concrete and has only the abstract declaration above it.
        assertEquals(JuxMethodState.SHOULD_DEFINE, states["Blob"])
    }

    fun testAnInheritedBodyIsNotAMissingOne() {
        myFixture.configureByText("a.jux", shapes)
        val structure = JuxMethodHierarchyTreeStructure(project, method("Shape", "draw"))
        val root = structure.rootElement as JuxMethodNodeDescriptor
        assertEquals("Drawable", root.type.name)
        val shape = structure.getChildElements(root).filterIsInstance<JuxMethodNodeDescriptor>().single()
        val states = structure.getChildElements(shape).filterIsInstance<JuxMethodNodeDescriptor>()
            .associate { it.type.name to it.state }
        assertEquals(JuxMethodState.DEFINED, states["Circle"])
        assertEquals(JuxMethodState.INHERITED, states["Square"])
    }

    fun testHidingTypesThatDoNotImplementIt() {
        myFixture.configureByText("a.jux", shapes)
        val state = HierarchyBrowserManager.getInstance(project).state!!
        val before = state.HIDE_CLASSES_WHERE_METHOD_NOT_IMPLEMENTED
        try {
            state.HIDE_CLASSES_WHERE_METHOD_NOT_IMPLEMENTED = true
            val structure = JuxMethodHierarchyTreeStructure(project, method("Shape", "draw"))
            val shape = structure.getChildElements(structure.rootElement)
                .filterIsInstance<JuxMethodNodeDescriptor>().single()
            val names = structure.getChildElements(shape).filterIsInstance<JuxMethodNodeDescriptor>().map { it.type.name }
            assertEquals(listOf("Circle"), names)
        } finally {
            state.HIDE_CLASSES_WHERE_METHOD_NOT_IMPLEMENTED = before
        }
    }

    // ---- helpers ------------------------------------------------------------

    private fun method(type: String, name: String): JuxMethodDeclaration {
        val decl = PsiTreeUtil.findChildrenOfType(myFixture.file, JuxTypeDeclaration::class.java).first { it.name == type }
        return PsiTreeUtil.findChildrenOfType(decl, JuxMethodDeclaration::class.java).first { it.name == name }
    }
}
