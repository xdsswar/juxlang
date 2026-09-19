package dev.jux.intellij.intentions

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.inspections.JuxAbiRulesInspection

/**
 * "Explain error EXXXX" is offered on a highlight whose message carries a Jux
 * code, and it would ask `juxc explain <code>`.
 */
class JuxExplainErrorIntentionTest : BasePlatformTestCase() {

    fun testCodeIsReadFromTheMessageShapesJuxUses() {
        assertEquals("E0413", JuxExplainErrorIntention.codeIn("[E0413] no method `add` on type `Vec`"))
        assertEquals("E0520", JuxExplainErrorIntention.codeIn("`@align` cannot be put on an enum (E0520)"))
        assertEquals("W0820", JuxExplainErrorIntention.codeIn("W0820: unsafe block without a SAFETY comment"))
        assertNull(JuxExplainErrorIntention.codeIn("Cannot resolve symbol 'x'"))
        assertNull(JuxExplainErrorIntention.codeIn(null))
        assertEquals(listOf("explain", "E0413"), JuxExplainErrorIntention.command("E0413"))
    }

    fun testOfferedOnACodedDiagnostic() {
        myFixture.enableInspections(JuxAbiRulesInspection())
        myFixture.configureByText("a.jux", "@al<caret>ign(64) enum Color { Red }\n")
        myFixture.doHighlighting()
        assertNotNull(myFixture.getAvailableIntention("Explain error E0520"))
    }

    fun testNotOfferedAwayFromDiagnostics() {
        myFixture.configureByText("a.jux", "void ma<caret>in() {}\n")
        myFixture.doHighlighting()
        assertTrue(myFixture.availableIntentions.none { it.text.startsWith("Explain error") })
    }
}
