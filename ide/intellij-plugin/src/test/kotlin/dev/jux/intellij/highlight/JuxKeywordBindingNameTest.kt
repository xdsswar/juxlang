package dev.jux.intellij.highlight

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/**
 * E0204 (ERRATA E131): a Jux keyword cannot name a parameter or a local.
 * The compiler reports it once and reads the keyword as the name everywhere
 * after, so nothing cascades; the editor parses the same way and reports the
 * same one error per binding (`tests/ui/keyword_binding_name.jux`).
 */
class JuxKeywordBindingNameTest : BasePlatformTestCase() {

    private val uiCase = """
        int area(int type, int record) {
            return type * record;
        }

        public void main() {
            int sealed = 2;
            var permits = 3;
            print(area(sealed, permits));
        }
    """.trimIndent()

    fun testEachKeywordBindingIsReportedOnceAndNothingCascades() {
        myFixture.configureByText("a.jux", uiCase)
        assertEquals(
            "no syntax errors",
            emptyList<String>(),
            PsiTreeUtil.findChildrenOfType(myFixture.file, PsiErrorElement::class.java).map { it.errorDescription },
        )
        val errors = myFixture.doHighlighting()
            .filter { it.severity === HighlightSeverity.ERROR }
            .mapNotNull { it.description }
        assertEquals(
            listOf(
                "'type' is a Jux keyword, so it cannot name a parameter (E0204)",
                "'record' is a Jux keyword, so it cannot name a parameter (E0204)",
                "'sealed' is a Jux keyword, so it cannot name a local variable (E0204)",
                "'permits' is a Jux keyword, so it cannot name a local variable (E0204)",
            ),
            errors,
        )
    }

    fun testTheKeywordReadsAsTheBinding() {
        myFixture.configureByText("a.jux", uiCase)
        val use = PsiTreeUtil.collectElements(myFixture.file) {
            it.elementType === E.REFERENCE_EXPRESSION && it.text == "record"
        }.single()
        val target = use.references.firstOrNull()?.resolve()
        assertEquals(E.PARAMETER, target?.elementType)
        assertEquals("record", (target as? JuxNamedElement)?.name)
    }

    fun testAStubKeepsItsRustNames() {
        myFixture.configureByText(
            "g37ui.jux.d",
            """
            package rust.g37ui;

            public class Ui {
                public void add(int type, String move);
            }
            """.trimIndent(),
        )
        assertTrue(myFixture.doHighlighting().none { it.description?.contains("E0204") == true })
    }

    fun testOrdinaryNamesAndTheReceiverAreFine() {
        myFixture.configureByText(
            "a.jux",
            """
            class Counter {
                int n = 0;
                void bump(Counter this, int kind) {
                    var typeName = kind;
                    n = n + typeName;
                }
            }
            """.trimIndent(),
        )
        assertTrue(myFixture.doHighlighting().none { it.description?.contains("E0204") == true })
    }
}
