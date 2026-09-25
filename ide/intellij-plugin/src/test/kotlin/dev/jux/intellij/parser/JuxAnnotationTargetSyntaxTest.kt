package dev.jux.intellij.parser

import com.intellij.psi.PsiElement
import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Annotations on parameters and locals (JUX-ANNOTATIONS-ADDENDUM §A.3.1,
 * ERRATA E104).
 *
 * §A.3 has always listed PARAMETER and LOCAL_VARIABLE among its targets, and
 * every framework pattern in §A.11 annotates a parameter: that is how a router
 * says which path segment fills an argument. The compiler learned both
 * positions; until it did, the editor had never had to parse one, and
 * `examples/annotations_param_local.jux` came out with `Unexpected '@'` and
 * `';' expected` on the local declaration in every method body. These tests
 * pin the whole surface so neither position can regress to that.
 */
class JuxAnnotationTargetSyntaxTest : BasePlatformTestCase() {

    private fun errors(): List<PsiErrorElement> =
        PsiTreeUtil.collectElementsOfType(myFixture.file, PsiErrorElement::class.java).toList()

    private fun parses(name: String, text: String) {
        myFixture.configureByText("$name.jux", text)
        val found = errors()
        assertTrue(
            "$name: " + found.joinToString { "${it.errorDescription} @${it.textOffset}" },
            found.isEmpty(),
        )
    }

    private fun nodes(type: com.intellij.psi.tree.IElementType): List<PsiElement> =
        PsiTreeUtil.collectElements(myFixture.file) { it.elementType === type }.toList()

    /** The `@…` prefixes that are DIRECT children of [owner], in source order. */
    private fun annotationsOf(owner: PsiElement): List<String> =
        owner.children.filter { it.elementType === E.ANNOTATION }.map { it.text }

    // ---- parameters --------------------------------------------------------

    fun testAnnotationsPrecedeEveryParameterBindingMode() {
        // §A.3.1: the prefix comes FIRST, ahead of `final` / `ref` / `weak` and
        // ahead of the type, and several may be stacked on one parameter.
        // `@Both final int` in that order is the shape the compiler accepts, so
        // it is the shape the editor must accept.
        parses(
            "modes",
            "void f(@Note(\"p\") int x, @Both ref int y, @Both final int z, " +
                "@Note @Both String who, @Note(\"v\") String... rest) {}",
        )
        assertEquals(5, nodes(E.PARAMETER).size)
        assertEquals(listOf("@Note(\"p\")"), annotationsOf(nodes(E.PARAMETER)[0]))
        assertEquals(listOf("@Note", "@Both"), annotationsOf(nodes(E.PARAMETER)[3]))
    }

    fun testEveryParameterListTakesThem() {
        // The four parameter lists the target PARAMETER reaches: a method, a
        // constructor, an interface's abstract method (no body, so its `)` is
        // followed by `;` rather than `{`), and a free function.
        parses(
            "lists",
            "interface Handler { String handle(@PathParam(\"id\") int id); }\n" +
                "class C implements Handler {\n" +
                "    C(@Audited @NotBlank(message = \"required\") String prefix) { this.prefix = prefix; }\n" +
                "    public String handle(@PathParam(\"id\") final int id) { return \"\" + id; }\n" +
                "}\n" +
                "void report(@Audited String tag, @NotBlank String... parts) { print(tag); }",
        )
        assertEquals(5, nodes(E.PARAMETER).size)
        assertTrue(nodes(E.PARAMETER).all { annotationsOf(it).isNotEmpty() })
    }

    // ---- locals ------------------------------------------------------------

    fun testAnnotationOnALocalDeclaration() {
        // Both spellings of a local, plus a named annotation argument: a `var`
        // and an explicitly typed one. LOCAL_VARIABLE is §A.3's only
        // statement-level target.
        parses(
            "locals",
            "void m(String p) {\n" +
                "    @Scratch(why = \"readability\") var label = p + \"/\";\n" +
                "    @Audited String out = \"\";\n" +
                "    @Both final int extra = 1;\n" +
                "    print(label + out + extra);\n" +
                "}",
        )
        assertEquals(3, nodes(E.LOCAL_VARIABLE).size)
    }

    fun testALocalInsideALambdaBodyTakesThem() {
        // The lambda's BODY is an ordinary block, so a local in it is annotated
        // like any other. Only the lambda's parameter list is out of bounds.
        parses(
            "inLambda",
            "void report(String tag) {\n" +
                "    var describe = () -> {\n" +
                "        @Scratch var n = tag.len();\n" +
                "        return tag + \"=\" + n;\n" +
                "    };\n" +
                "    print(describe());\n" +
                "}",
        )
        assertEquals(2, nodes(E.LOCAL_VARIABLE).size)
    }

    fun testTheAnnotationIsInsideTheDeclarationItPrecedes() {
        // The prefix belongs to the LOCAL_VARIABLE node, not beside it in the
        // block: that is where `children.filter { it is ANNOTATION }` readers
        // and the formatter's "annotations align with the declaration they
        // precede" indent rule look for it. Left as a sibling, the annotated
        // local would also re-indent as a statement of its own.
        parses("inside", "void m() { @Scratch(why = \"x\") var total = 0; print(total); }")
        val local = nodes(E.LOCAL_VARIABLE).single()
        assertEquals(listOf("@Scratch(why = \"x\")"), annotationsOf(local))
        assertEquals(E.CODE_BLOCK, local.parent.elementType)
        assertTrue(annotationsOf(local.parent).isEmpty())
    }

    // ---- the boundary ------------------------------------------------------

    fun testALambdaParameterAnnotationIsStillRefused() {
        // A lambda parameter is NOT a `param`: Grammar §A.2.9 is
        // `lambda-param = type? identifier`, with no annotation slot, so the
        // compiler refuses `(@Audited int a) -> a` and the editor has to agree.
        // Accepting it here is how the IDE ends up greenlighting code that then
        // fails to build.
        myFixture.configureByText("lambdaParam.jux", "void m() { var f = (@Audited int a) -> a; print(f); }")
        assertFalse("a lambda parameter must not take an annotation", errors().isEmpty())
        assertTrue("nothing should have parsed as a lambda", nodes(E.LAMBDA_EXPRESSION).isEmpty())
    }

    fun testAMisplacedStatementAnnotationDoesNotBreakTheBlock() {
        // A prefix on a non-declaration statement is a WRONG TARGET, which juxc
        // reports precisely; it is not a syntax error. Reporting it here too
        // would paint the rest of the body red over a diagnostic the build
        // already gives, so the statement still parses and the block is intact.
        parses("wrongTarget", "void m() { @Audited print(1); print(2); }")
        assertEquals(2, nodes(E.EXPRESSION_STATEMENT).size)
        assertEquals(1, nodes(E.ANNOTATION).size)
    }
}
