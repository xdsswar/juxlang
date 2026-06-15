package dev.jux.intellij.psi

import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * Offline awareness of the implicit-nullable auto-property rule (§M.7.3.1): an
 * auto-property with no initializer is implicitly nullable (`T?`) and defaults
 * to null. [JuxPropertyDeclaration.isImplicitlyNullable] / [effectiveTypeText]
 * mirror the compiler's desugar so completion + hover show the real `T?` type
 * without an LSP session.
 */
class JuxPropertyNullableTest : BasePlatformTestCase() {

    private fun props(code: String): Map<String, JuxPropertyDeclaration> {
        myFixture.configureByText("a.jux", code)
        return PsiTreeUtil.findChildrenOfType(myFixture.file, JuxPropertyDeclaration::class.java)
            .associateBy { it.name!! }
    }

    fun testImplicitNullableRules() {
        val p = props(
            """
            package demo;
            public class C {
                public int Auto { get; set; }
                public int Initialized { get; set; } = 0;
                public int? AlreadyNullable { get; set; }
                public int Computed { get -> 1; }
            }
            """.trimIndent(),
        )

        // Uninitialized auto-property → implicitly nullable, reads `int?`.
        assertTrue(p["Auto"]!!.isImplicitlyNullable())
        assertEquals("int?", p["Auto"]!!.effectiveTypeText())

        // A real initializer keeps the declared, non-nullable type.
        assertFalse(p["Initialized"]!!.isImplicitlyNullable())
        assertEquals("int", p["Initialized"]!!.effectiveTypeText())

        // Already nullable → not transformed; effective type stays `int?`.
        assertFalse(p["AlreadyNullable"]!!.isImplicitlyNullable())
        assertEquals("int?", p["AlreadyNullable"]!!.effectiveTypeText())

        // Computed (expression-bodied) accessor → no backing field → unaffected.
        assertFalse(p["Computed"]!!.isImplicitlyNullable())
        assertEquals("int", p["Computed"]!!.effectiveTypeText())
    }
}
