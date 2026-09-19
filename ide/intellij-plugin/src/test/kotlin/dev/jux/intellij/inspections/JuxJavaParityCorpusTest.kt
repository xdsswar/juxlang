package dev.jux.intellij.inspections

import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.JuxCorpus
import java.io.File

/**
 * The Java-parity inspections over every `.jux` in `examples/`: none of them
 * may report ANYTHING a user would see (weak warning and up) on the corpus.
 *
 * [JuxCorpusHighlightingTest] only asserts "no errors", which leaves room for
 * a warning inspection to paint yellow over code that is fine. These checks
 * are the kind a user switches off after the third false alarm, so the bar
 * here is higher: the examples are correct, idiomatic Jux, and every finding
 * on them is a bug in the inspection. Information-level checks (Java's
 * "Redundant 'else'") never highlight and are not counted.
 */
class JuxJavaParityCorpusTest : BasePlatformTestCase() {

    override fun getTestDataPath(): String = File("../../examples").absolutePath

    private val inspections = listOf(
        JuxEmptyCatchBlockInspection(),
        JuxRedundantCastInspection(),
        JuxConstantConditionInspection(),
        JuxRedundantElseInspection(),
        JuxPointlessBooleanExpressionInspection(),
        JuxUnusedPrivateMemberInspection(),
        JuxDuplicateConditionInspection(),
        JuxSelfAssignmentInspection(),
        JuxEmptyStatementBodyInspection(),
        JuxMissingReturnInspection(),
        JuxGeneratorInspection(),
        JuxInterfaceOperatorInspection(),
        JuxSimplifiableIfInspection(),
        JuxIndexedLoopInspection(),
        JuxAbstractMethodInClassInspection(),
        JuxUnhandledExceptionInspection(),
    )

    override fun setUp() {
        super.setUp()
        myFixture.enableInspections(*inspections.toTypedArray())
    }

    /**
     * Findings that are correct: compiler regression examples that write a
     * constant expression ON PURPOSE, to test how the compiler lowers it. Java
     * reports the same code. Keyed "file:inspection:line"; anything not
     * listed fails the test.
     */
    private val expected = setOf(
        // `print('a' < 'b')`: char comparison lowering.
        "char_arithmetic.jux:JuxConstantCondition:23",
        // `print((a < b) == true)`: parenthesized operands.
        "fn_type_locals.jux:JuxPointlessBooleanExpression:31",
        // `true ? 1 : 2.0`: mixed-arm numeric promotion.
        "numeric_literals_and_promotion.jux:JuxConstantCondition:44",
        "numeric_literals_and_promotion.jux:JuxConstantCondition:46",
        "numeric_literals_and_promotion.jux:JuxConstantCondition:49",
        // `while (Tape.advance());`: the empty statement as a loop body is
        // what the example demonstrates (Grammar §A.2.8).
        "labeled_blocks.jux:JuxEmptyStatementBody:48",
        // `for (var i : 0..values.length) { if (test(values[i])) ... }`: the
        // lesson keeps its indexed loop on purpose, which is exactly what
        // the check reports (a for-each would do).
        "../tests/lessons/Callback/src/Main.jux:JuxIndexedLoop:37",
    )

    fun testCorpusHasNoFindings() {
        val examples = File(testDataPath)
        assertTrue("examples dir not found at ${examples.absolutePath}", examples.isDirectory)
        val ids = inspections.map { it.shortName }.toSet()
        val files = JuxCorpus.sweep(examples)
        // All files first, so cross-file lookups see the whole project.
        val added = files.map { myFixture.addFileToProject(it.name, it.file.readText()) }
        val failures = StringBuilder()
        val seen = HashSet<String>()
        for (i in files.indices) {
            myFixture.configureFromExistingVirtualFile(added[i].virtualFile)
            val text = myFixture.editor.document.text
            val rel = files[i].file.relativeTo(examples).path.replace('\\', '/')
            val findings = myFixture.doHighlighting()
                .filter { it.inspectionToolId in ids && it.severity > HighlightSeverity.INFORMATION }
                .map { it to text.substring(0, it.startOffset).count { c -> c == '\n' } + 1 }
                .filter { (info, line) ->
                    val key = "$rel:${info.inspectionToolId}:$line"
                    seen.add(key)
                    key !in expected
                }
            if (findings.isNotEmpty()) {
                failures.appendLine("- $rel:")
                findings.take(6).forEach { (info, line) ->
                    failures.appendLine("    [${info.inspectionToolId}] line $line: ${info.description}")
                }
            }
        }
        assertTrue("findings on correct code:\n$failures", failures.isEmpty())
        // The expected list must not rot: each entry is still found.
        assertEquals("expected findings no longer reported", emptySet<String>(), expected - seen)
    }
}
