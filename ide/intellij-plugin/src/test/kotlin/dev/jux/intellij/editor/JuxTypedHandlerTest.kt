package dev.jux.intellij.editor

import com.intellij.testFramework.fixtures.BasePlatformTestCase

/**
 * [JuxTypedHandler] — smart interpolation-string typing. Types a single char
 * into a snippet with a `<caret>` marker and asserts the resulting document
 * text and caret placement. The assertions hold whether the close char comes
 * from the typed handler or (for `"`) the platform quote handler, since the end
 * state is the same single balanced pair.
 */
class JuxTypedHandlerTest : BasePlatformTestCase() {

    private val D = '$' // keeps Kotlin string interpolation out of the snippets

    fun testDollarQuoteAutoCloses() {
        myFixture.configureByText("a.jux", "class C { void m() { print(${D}<caret>); } }")
        myFixture.type('"')
        val text = myFixture.editor.document.text
        val caret = myFixture.caretOffset
        assertTrue("expected an auto-closed \$\"\" pair, got: $text", text.contains("print(${D}\"\")"))
        assertEquals('"', text[caret - 1]) // open quote on the left
        assertEquals('"', text[caret])     // close quote on the right (caret is between)
    }

    fun testPlainQuoteUnaffectedByDelegate() {
        // A normal `"` (no preceding `$`) still auto-closes via the quote
        // handler — the delegate must not interfere or double up.
        myFixture.configureByText("a.jux", "class C { void m() { print(<caret>); } }")
        myFixture.type('"')
        val text = myFixture.editor.document.text
        assertTrue("expected a single \"\" pair, got: $text", text.contains("print(\"\")"))
        assertFalse("no triple quote", text.contains("\"\"\""))
    }

    fun testDollarBraceAutoClosesInsideInterpString() {
        // Caret sits right after a `$` already inside an interpolation literal;
        // typing `{` opens a hole that should auto-close.
        myFixture.configureByText("a.jux", "class C { void m() { var s = ${D}\"a${D}<caret>\"; } }")
        myFixture.type('{')
        val text = myFixture.editor.document.text
        val caret = myFixture.caretOffset
        assertTrue("expected an auto-closed \${} hole, got: $text", text.contains("${D}\"a${D}{}\""))
        assertEquals('{', text[caret - 1])
        assertEquals('}', text[caret])
    }
}
