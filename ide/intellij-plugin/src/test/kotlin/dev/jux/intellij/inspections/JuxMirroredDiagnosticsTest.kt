package dev.jux.intellij.inspections

import com.intellij.openapi.util.TextRange
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import java.io.File

/**
 * The LSP dedup: a server diagnostic is dropped only when the plugin reports
 * the same code over an overlapping range, and [JuxMirroredDiagnostics.MIRRORED]
 * cannot drift from the codes the plugin's checks actually print.
 */
class JuxMirroredDiagnosticsTest : BasePlatformTestCase() {

    private val implementsAClass = """
        class Base { }
        class Other { }
        class Wrong implements Base { }
        int f(int type) { return type; }
    """.trimIndent()

    private fun rangeOf(text: String): TextRange {
        val start = myFixture.file.text.indexOf(text)
        assertTrue("'$text' not found", start >= 0)
        return TextRange(start, start + text.length)
    }

    fun testTheSameCodeAtTheSamePlaceIsADuplicate() {
        myFixture.enableInspections(JuxImplementsClauseInspection())
        myFixture.configureByText("a.jux", implementsAClass)
        val file = myFixture.file
        // The compiler's E0424 span: the whole `implements` entry.
        assertTrue(JuxMirroredDiagnostics.isDuplicate(file, "E0424", rangeOf("implements Base")))
        // The same code somewhere the plugin says nothing: the server's is kept.
        assertFalse(JuxMirroredDiagnostics.isDuplicate(file, "E0424", rangeOf("class Other")))
        // Another code at the same place, and a code the plugin never reports.
        assertFalse(JuxMirroredDiagnostics.isDuplicate(file, "E0429", rangeOf("implements Base")))
        assertFalse(JuxMirroredDiagnostics.isDuplicate(file, "E0900", rangeOf("implements Base")))
        assertFalse(JuxMirroredDiagnostics.isDuplicate(file, null, rangeOf("implements Base")))
    }

    fun testAnAnnotatorCodeIsADuplicate() {
        myFixture.configureByText("a.jux", implementsAClass)
        assertTrue(JuxMirroredDiagnostics.isDuplicate(myFixture.file, "E0204", rangeOf("type)")))
        assertFalse(JuxMirroredDiagnostics.isDuplicate(myFixture.file, "E0204", rangeOf("class Base")))
    }

    fun testASwitchedOffInspectionLeavesTheServersCopy() {
        // Nothing enabled: the plugin shows no E0424, so the server's must stay.
        myFixture.configureByText("a.jux", implementsAClass)
        assertFalse(JuxMirroredDiagnostics.isDuplicate(myFixture.file, "E0424", rangeOf("implements Base")))
    }

    fun testCodeAndRangeFromAnLspDiagnostic() {
        assertEquals("E0424", JuxMirroredDiagnostics.codeOf("E0424", "anything"))
        assertEquals("W0491", JuxMirroredDiagnostics.codeOf(null, "a.jux:3:5: [W0491] warning: `f` is deprecated"))
        assertNull(JuxMirroredDiagnostics.codeOf("not-a-code", "no code here"))
        myFixture.configureByText("a.jux", implementsAClass)
        val document = myFixture.editor.document
        val range = JuxMirroredDiagnostics.textRange(document, 2, 12, 2, 27)
        assertEquals("implements Base", range?.substring(document.text))
    }

    /**
     * Every `(E0xxx)` / `(W0xxx)` an inspection or annotator prints is in the
     * list, under the inspection that prints it: a check that starts
     * reporting a new code without a line in [JuxMirroredDiagnostics.SOURCES]
     * would show its error twice while the language server runs.
     */
    fun testTheListCoversEveryCodeThePluginPrints() {
        val root = File("src/main/kotlin/dev/jux/intellij")
        assertTrue(root.absolutePath, root.isDirectory)
        val code = Regex("""\(([EW]0\d{3})\)""")
        val byShortName = JuxMirroredDiagnostics.SOURCES.associateBy { it.shortName }
        val missing = ArrayList<String>()
        for (dir in listOf("inspections", "highlight")) {
            File(root, dir).listFiles { f -> f.name.endsWith(".kt") }!!.forEach { f ->
                if (f.name == "JuxMirroredDiagnostics.kt") return@forEach
                val codes = code.findAll(f.readText()).map { it.groupValues[1] }.toSet()
                if (codes.isEmpty()) return@forEach
                val shortName = if (dir == "highlight") null else f.name.removeSuffix(".kt").removeSuffix("Inspection")
                val listed = byShortName[shortName]?.codes.orEmpty()
                (codes - listed).forEach { missing.add("${f.name}: $it") }
            }
        }
        assertEquals(emptyList<String>(), missing)
    }

    fun testEverySourceIsARegisteredInspection() {
        val xml = File("src/main/resources/META-INF/plugin.xml").readText()
        for (source in JuxMirroredDiagnostics.SOURCES) {
            val name = source.shortName ?: continue
            assertTrue(name, xml.contains("shortName=\"$name\""))
            assertEquals(name, source.create!!().shortName)
        }
    }
}
