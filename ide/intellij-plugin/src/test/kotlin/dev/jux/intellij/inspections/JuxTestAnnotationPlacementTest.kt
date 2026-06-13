package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/** §TS.1 placement enforcement — free functions, no params, void return. */
class JuxTestAnnotationPlacementTest : BasePlatformTestCase() {

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(JuxTestAnnotationPlacementInspection())
    }

    private fun highlightDescriptions(code: String): List<String> {
        myFixture.configureByText("t.jux", code)
        return myFixture.doHighlighting().mapNotNull { it.description }
    }

    fun testOnClassMethodFlagged() {
        val d = highlightDescriptions(
            """
            public class A {
                @Test
                public void m() {}
            }
            """.trimIndent(),
        )
        assertTrue(d.any { it.contains("only valid on free functions") })
    }

    fun testWithParametersFlagged() {
        val d = highlightDescriptions(
            """
            @Test
            void m(int x) {}
            """.trimIndent(),
        )
        assertTrue(d.any { it.contains("must take no parameters") })
    }

    fun testNonVoidReturnFlagged() {
        val d = highlightDescriptions(
            """
            @BeforeEach
            int setup() { return 1; }
            """.trimIndent(),
        )
        assertTrue(d.any { it.contains("must return void") })
    }

    fun testValidAsyncVoidTestIsClean() {
        val d = highlightDescriptions(
            """
            package demo;
            @Test
            async void ok() {}
            @AfterAll
            void done() {}
            """.trimIndent(),
        )
        assertFalse(d.any { it.contains("TS.1") })
    }

    fun testRemoveAnnotationQuickFix() {
        myFixture.configureByText(
            "t.jux",
            """
            public class A {
                @Te<caret>st
                public void m() {}
            }
            """.trimIndent(),
        )
        myFixture.doHighlighting()
        val fix = myFixture.findSingleIntention("Remove @Test annotation")
        myFixture.launchAction(fix)
        assertFalse(myFixture.file.text.contains("@Test"))
        assertTrue(myFixture.file.text.contains("public void m()"))
    }
}
