package dev.jux.intellij.run

import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxMethodDeclaration

/**
 * §TS.1 detection: free-function gating, case-insensitive annotations, and
 * the package-qualified display names the runner prints (§TS.2).
 */
class JuxTestDetectorTest : BasePlatformTestCase() {

    private fun methods(code: String): List<JuxMethodDeclaration> {
        val file = myFixture.configureByText("t.jux", code)
        return PsiTreeUtil.findChildrenOfType(file, JuxMethodDeclaration::class.java).toList()
    }

    fun testFreeFunctionWithTestAnnotation() {
        val m = methods(
            """
            package demo;
            @Test
            void works() {}
            """.trimIndent(),
        ).single()
        assertTrue(JuxTestDetector.isTestFunction(m))
        assertTrue(JuxTestDetector.isTestOrHookFunction(m))
        assertEquals("demo.works", JuxTestDetector.qualifiedName(m))
    }

    fun testAnnotationCasingIsIrrelevant() {
        val ms = methods(
            """
            @test
            void lower() {}
            @TEST
            void upper() {}
            @BeForeEaCh
            void mixed() {}
            """.trimIndent(),
        )
        assertTrue(JuxTestDetector.isTestFunction(ms[0]))
        assertTrue(JuxTestDetector.isTestFunction(ms[1]))
        assertFalse(JuxTestDetector.isTestFunction(ms[2])) // hook, not a test
        assertTrue(JuxTestDetector.isTestOrHookFunction(ms[2]))
    }

    fun testClassMethodIsNeverATest() {
        val m = methods(
            """
            public class Holder {
                @Test
                public void notATest() {}
            }
            """.trimIndent(),
        ).single()
        assertFalse(JuxTestDetector.isFreeFunction(m))
        assertFalse(JuxTestDetector.isTestFunction(m))
    }

    fun testQualifiedNameWithoutPackage() {
        val m = methods(
            """
            @Test
            void bare() {}
            """.trimIndent(),
        ).single()
        assertEquals("bare", JuxTestDetector.qualifiedName(m))
    }

    fun testHasTestsAndRegexGate() {
        val code = """
            package p;
            @Test
            void one() {}
        """.trimIndent()
        assertTrue(JuxTestDetector.hasTestsText(code))
        val file = myFixture.configureByText("t.jux", code)
        assertTrue(JuxTestDetector.hasTests(file))

        val none = """
            package p;
            void plain() {}
        """.trimIndent()
        assertFalse(JuxTestDetector.hasTestsText(none))
    }
}
