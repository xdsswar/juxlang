package dev.jux.intellij.editor

import junit.framework.TestCase

/**
 * [JuxImportSupport.interpolatedNames] — the extraction that keeps imports and
 * locals used ONLY inside `$"…"` interpolation holes from being flagged (and
 * deleted) as unused. The interp literal is one lexer token, so this text scan
 * is the only visibility the inspections have into it.
 */
class JuxInterpolatedNamesTest : TestCase() {

    fun testHoleAndShorthandNamesAreExtracted() {
        val names = JuxImportSupport.interpolatedNames(
            "\$\"sum=\${a + b.size()} who=\$who\"",
            raw = false,
        )
        assertTrue("hole identifiers", names.containsAll(setOf("a", "b", "size")))
        assertTrue("\$name shorthand", names.contains("who"))
    }

    fun testCookedEscapedDollarIsNotAHole() {
        val names = JuxImportSupport.interpolatedNames("\$\"price \\\${cost}\"", raw = false)
        assertFalse("\\\${…} is literal text in the cooked form", names.contains("cost"))
    }

    fun testRawBackslashDollarIsAHole() {
        // In `$"""…"""` the backslash is plain text, so `\${name}` interpolates.
        val names = JuxImportSupport.interpolatedNames(
            "\$\"\"\"C:\\dir\\\${name}\"\"\"",
            raw = true,
        )
        assertTrue("raw \\\${…} IS a hole", names.contains("name"))
    }

    fun testNestedBracesStayInsideTheHole() {
        val names = JuxImportSupport.interpolatedNames(
            "\$\"v=\${ xs.map(x -> { x.total }) } done\"",
            raw = false,
        )
        assertTrue(names.containsAll(setOf("xs", "map", "x", "total")))
        assertFalse("text after the hole is not collected", names.contains("done"))
    }

    fun testUnterminatedHoleCollectsWhatIsThere() {
        val names = JuxImportSupport.interpolatedNames("\$\"v=\${count", raw = false)
        assertTrue("mid-edit hole still counts its names", names.contains("count"))
    }
}
