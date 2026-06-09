package dev.jux.intellij.parser

import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.testFramework.ParsingTestCase
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxEnumConstant
import dev.jux.intellij.psi.JuxElementTypes
import dev.jux.intellij.psi.JuxFieldDeclaration
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxParserDefinition
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxReference
import java.io.File

/**
 * Parses every `.jux` file in the repository's `examples/` corpus through the
 * real [JuxParser] and asserts the resulting PSI tree has no [PsiErrorElement]s.
 *
 * This is the lenient-superset acceptance bar from the implementation plan: the
 * declaration-level parser must accept the whole corpus without red squiggles
 * (method bodies are opaque, so the bar covers the compilation unit, type
 * declarations, members, and signatures).
 */
class JuxParsingTest : ParsingTestCase("", "jux", JuxParserDefinition()) {

    fun testAllExamplesParseWithoutErrors() {
        val examples = File(testDataPath)
        assertTrue("examples dir not found at ${examples.absolutePath}", examples.isDirectory)

        val failures = StringBuilder()
        var count = 0
        examples.listFiles { _, name -> name.endsWith(".jux") }!!.sortedBy { it.name }.forEach { file ->
            count++
            val psi = createPsiFile(file.name, file.readText())
            val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
            if (errors.isNotEmpty()) {
                failures.appendLine("• ${file.name}:")
                errors.take(5).forEach { failures.appendLine("    ${it.errorDescription} @ ${it.textOffset}") }
            }
        }
        assertTrue("parsed $count files", count > 0)
        assertTrue("parse errors found:\n$failures", failures.isEmpty())
    }

    /** Validates the named-declaration PSI that Structure View / navigation use. */
    fun testPsiNamesAndMembers() {
        val psi = createPsiFile(
            "Sample.jux",
            """
            package demo;
            public class Greeter {
                private int count;
                public void greet(String who) { print(who); }
            }
            public enum Color { Red, Green, Blue }
            """.trimIndent(),
        )
        assertEmpty(PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java))

        val types = PsiTreeUtil.collectElementsOfType(psi, JuxTypeDeclaration::class.java).map { it.name }.toSet()
        assertEquals(setOf("Greeter", "Color"), types)

        val methods = PsiTreeUtil.collectElementsOfType(psi, JuxMethodDeclaration::class.java).map { it.name }
        assertTrue("greet method", methods.contains("greet"))

        val fields = PsiTreeUtil.collectElementsOfType(psi, JuxFieldDeclaration::class.java).map { it.name }
        assertTrue("count field", fields.contains("count"))

        val constants = PsiTreeUtil.collectElementsOfType(psi, JuxEnumConstant::class.java).map { it.name }.toSet()
        assertEquals(setOf("Red", "Green", "Blue"), constants)
    }

    /**
     * The PSI structure the override-methods Generate action relies on: a
     * class's `implements`/`extends` clause exposes its supertype names as
     * TYPE_REFERENCE nodes, and the class body holds its methods as
     * METHOD_DECLARATION children — so the action can read supertype names and
     * method signatures off the tree.
     */
    fun testOverrideActionPsiShape() {
        val psi = createPsiFile(
            "Shape.jux",
            """
            package demo;
            public interface Shape { double area(); String name(); }
            public class Circle implements Shape {
                public double area() { return 3.14; }
            }
            """.trimIndent(),
        )
        assertEmpty(PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java))

        val circle = PsiTreeUtil.collectElementsOfType(psi, JuxTypeDeclaration::class.java)
            .first { it.name == "Circle" }
        // implements clause carries the supertype reference `Shape`.
        val impl = circle.node.findChildByType(dev.jux.intellij.psi.JuxElementTypes.IMPLEMENTS_CLAUSE)
        assertNotNull("implements clause present", impl)
        val refs = PsiTreeUtil.collectElements(impl!!.psi) {
            it.node.elementType == dev.jux.intellij.psi.JuxElementTypes.TYPE_REFERENCE
        }.map { it.text.trim() }
        assertTrue("supertype Shape referenced", refs.any { it.contains("Shape") })

        // The class body exposes its own method as a METHOD_DECLARATION child.
        val body = circle.node.findChildByType(dev.jux.intellij.psi.JuxElementTypes.CLASS_BODY)
        assertNotNull("class body present", body)
        val ownMethods = body!!.psi.children
            .filter { it.node.elementType == dev.jux.intellij.psi.JuxElementTypes.METHOD_DECLARATION }
            .mapNotNull { (it as? dev.jux.intellij.psi.JuxNamedElement)?.name }
        assertTrue("Circle declares area()", ownMethods.contains("area"))
    }

    /** A use of a type name resolves to its in-file declaration. */
    fun testReferenceResolvesToDeclaration() {
        val file = createPsiFile(
            "Ref.jux",
            """
            public class Box { public int size() { return 0; } }
            public class User { public void run() { var b = new Box(); } }
            """.trimIndent(),
        )
        // The `Box` identifier inside `new Box()` lives under a TYPE_REFERENCE.
        val use = file.firstChild.let { collectIdentifiers(file) }
            .first { it.text == "Box" && it.parent.elementType === JuxElementTypes.TYPE_REFERENCE }
        val resolved = JuxReference(use, TextRange(0, use.textLength)).resolve()
        assertTrue("resolves to the Box class", resolved is JuxTypeDeclaration && (resolved as JuxNamedElement).name == "Box")
    }

    private fun collectIdentifiers(root: PsiElement): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        PsiTreeUtil.processElements(root) { e ->
            if (e.elementType === JuxTokenTypes.IDENTIFIER) out.add(e)
            true
        }
        return out
    }

    // Examples live at <repo>/examples; the plugin module is <repo>/ide/intellij-plugin.
    override fun getTestDataPath(): String = File("../../examples").absolutePath

    override fun skipSpaces(): Boolean = false
    override fun includeRanges(): Boolean = true
}
