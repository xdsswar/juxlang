package dev.jux.intellij.inspections

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import java.io.File

/**
 * Every `.jux` file in the repository's `examples/` corpus, run through the
 * annotator and every inspection the plugin ships, asserting **no
 * error-severity highlight**.
 *
 * `JuxParsingTest` already proves the corpus PARSES. This is the other half:
 * the corpus compiles, so any red the plugin paints on it is a plugin bug, and
 * red on correct code is the single most damaging thing an IDE can do — it
 * trains the user to ignore the editor. Every example is therefore a
 * regression test for the whole semantic surface at once, which is the only
 * scalable way to keep the plugin level with a language that is still growing.
 *
 * Files are added to ONE project, so the bare names they declare collide the
 * way a real multi-file project's do — `Tagged` is an interface in one example
 * and a class in two others. Resolving a name in its own file first is what
 * makes that correct rather than a source of confident, wrong errors.
 */
class JuxCorpusHighlightingTest : BasePlatformTestCase() {

    override fun getTestDataPath(): String = File("../../examples").absolutePath

    override fun setUp() {
        super.setUp()
        // Every inspection the plugin ships. Only ERROR severity is asserted
        // below, but running all of them over 230 real files is also the only
        // broad check that none of them throws on shapes the tests do not
        // enumerate — an inspection that crashes takes the whole daemon pass
        // with it, so the user sees no highlighting at all rather than a bug.
        myFixture.enableInspections(
            JuxAbstractNotImplementedInspection(),
            JuxAccessorVisibilityInspection(),
            JuxBindTypeMismatchInspection(),
            JuxBoundPropertyAssignmentInspection(),
            JuxExtendsClauseInspection(),
            JuxImplementsClauseInspection(),
            JuxInheritedTypeParamInspection(),
            JuxMisplacedAccessorBlockInspection(),
            JuxMissingOverrideInspection(),
            JuxPropertyNamingInspection(),
            JuxPropertyNeverObservedInspection(),
            JuxRedundantSemicolonInspection(),
            JuxSetterEarlyReturnInspection(),
            JuxTestAnnotationPlacementInspection(),
            JuxUnreachableCodeInspection(),
            JuxUnresolvedReferenceInspection(),
            JuxUnusedImportInspection(),
            JuxUnusedLocalSymbolInspection(),
        )
    }

    fun testCorpusHasNoErrorHighlights() {
        val examples = File(testDataPath)
        assertTrue("examples dir not found at ${examples.absolutePath}", examples.isDirectory)

        val failures = StringBuilder()
        var count = 0
        val files = examples.listFiles { _, name -> name.endsWith(".jux") }!!.sortedBy { it.name }
        for (file in files) {
            count++
            myFixture.configureByText(file.name, file.readText())
            val errors = myFixture.doHighlighting()
                .filter { it.severity === HighlightSeverity.ERROR }
                .mapNotNull { it.description }
                .distinct()
            if (errors.isNotEmpty()) {
                failures.appendLine("- ${file.name}:")
                errors.take(4).forEach { failures.appendLine("    $it") }
            }
        }
        assertTrue("no examples found", count > 0)
        assertTrue("error highlights on code that compiles:\n$failures", failures.isEmpty())
    }
}
