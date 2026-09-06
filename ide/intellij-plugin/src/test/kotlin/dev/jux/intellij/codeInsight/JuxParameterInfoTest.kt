package dev.jux.intellij.codeInsight

import com.intellij.psi.PsiElement
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import com.intellij.testFramework.utils.parameterInfo.MockCreateParameterInfoContext
import com.intellij.testFramework.utils.parameterInfo.MockUpdateParameterInfoContext

/**
 * [JuxParameterInfoHandler] — the `Ctrl+P` popup.
 *
 * Drives the handler through the platform's own mock contexts, which is the
 * same path the real action takes, so a signature that renders here is the one
 * that renders in the editor.
 */
class JuxParameterInfoTest : BasePlatformTestCase() {

    private val handler = JuxParameterInfoHandler()

    fun testOffersTheSignatureOfAMethodCall() {
        val signatures = signaturesAtCaret(
            """
            class Greeter {
                void greet(String who, int times) { }
                void run() { greet(<caret>); }
            }
            """.trimIndent(),
        )
        assertEquals(1, signatures.size)
        assertEquals("String who, int times", signatures[0].label)
    }

    fun testEveryOverloadIsOffered() {
        // Jux overloads on parameter TYPES, so two same-arity candidates must
        // both survive -- a name+arity dedup would drop one silently.
        val signatures = signaturesAtCaret(
            """
            class Printer {
                void show(int n) { }
                void show(String s) { }
                void run() { show(<caret>); }
            }
            """.trimIndent(),
        )
        assertEquals(2, signatures.size)
        assertTrue(signatures.any { it.label == "int n" })
        assertTrue(signatures.any { it.label == "String s" })
    }

    fun testConstructorArgumentsAreOffered() {
        val signatures = signaturesAtCaret(
            """
            class Point {
                Point(int x, int y) { }
            }
            class Uses {
                void run() { var p = new Point(<caret>); }
            }
            """.trimIndent(),
        )
        assertEquals(1, signatures.size)
        assertEquals("int x, int y", signatures[0].label)
    }

    fun testAMemberCallThroughAReceiverResolves() {
        val signatures = signaturesAtCaret(
            """
            class Engine {
                void start(int rpm) { }
            }
            class Car {
                Engine engine;
                void run() { engine.start(<caret>); }
            }
            """.trimIndent(),
        )
        assertEquals(1, signatures.size)
        assertEquals("int rpm", signatures[0].label)
    }

    fun testActiveParameterFollowsTheCommas() {
        assertEquals(0, activeParameterAt("greet(<caret>)"))
        assertEquals(1, activeParameterAt("greet(1, <caret>)"))
        assertEquals(2, activeParameterAt("greet(1, 2, <caret>)"))
    }

    fun testACommaInsideAnArgumentDoesNotAdvanceTheParameter() {
        // The comma in `pair(2, 3)` belongs to that call, not to the outer one
        // -- counting it would highlight `greet`'s fourth parameter here
        // instead of its third.
        assertEquals(2, activeParameterAt("greet(1, pair(2, 3), <caret>)"))
    }

    fun testNoPopupOutsideACall() {
        val file = """
            class C {
                void m() { var x = 1;<caret> }
            }
        """.trimIndent()
        myFixture.configureByText("a.jux", file)
        val context = MockCreateParameterInfoContext(myFixture.editor, myFixture.file)
        assertNull(handler.findElementForParameterInfo(context))
    }

    // ---- helpers -----------------------------------------------------------

    private fun signaturesAtCaret(source: String): List<JuxParameterInfoHandler.Signature> {
        myFixture.configureByText("a.jux", source)
        val context = MockCreateParameterInfoContext(myFixture.editor, myFixture.file)
        val owner = handler.findElementForParameterInfo(context)
        assertNotNull("expected the caret to be inside an argument list", owner)
        @Suppress("UNCHECKED_CAST")
        return (context.itemsToShow ?: emptyArray()).toList() as List<JuxParameterInfoHandler.Signature>
    }

    /** The active parameter index the handler reports for a snippet. */
    private fun activeParameterAt(call: String): Int {
        myFixture.configureByText(
            "a.jux",
            """
            class C {
                void greet(int a, int b, int c) { }
                int pair(int a, int b) { return a; }
                void m() { $call; }
            }
            """.trimIndent(),
        )
        val create = MockCreateParameterInfoContext(myFixture.editor, myFixture.file)
        val owner: PsiElement = handler.findElementForParameterInfo(create)
            ?: throw AssertionError("expected an argument list at the caret in: $call")
        val update = MockUpdateParameterInfoContext(myFixture.editor, myFixture.file)
        handler.updateParameterInfo(owner, update)
        return update.currentParameter
    }
}
