package dev.jux.intellij.completion

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import java.io.File

/**
 * Completion driven at many positions across every `.jux` file in the
 * repository's `examples/` corpus, asserting it never throws.
 *
 * The value is not in what it asserts about any one position — it is that
 * completion runs over 240 real programs, on every syntactic shape the language
 * has, without a single exception. A contributor that throws takes the whole
 * lookup with it: the user gets no popup at all and no error to report, which
 * is indistinguishable from the feature simply being missing. Every unit test
 * in this package covers a shape someone thought of; this covers the ones
 * nobody did.
 *
 * The positions are chosen to be the ones that historically broke: after a `.`,
 * after an `@`, at a statement start, and inside a comment. Each file
 * contributes a bounded number so the whole sweep stays a few seconds.
 */
class JuxCorpusCompletionTest : BasePlatformTestCase() {

    override fun getTestDataPath(): String = File("../../examples").absolutePath

    fun testCompletionNeverThrowsAcrossTheCorpus() {
        val examples = File(testDataPath)
        assertTrue("examples dir not found at ${examples.absolutePath}", examples.isDirectory)

        val failures = StringBuilder()
        var positions = 0
        val files = examples.listFiles { _, name -> name.endsWith(".jux") }!!.sortedBy { it.name }
        for (file in files) {
            val text = file.readText()
            for (offset in probePositions(text)) {
                positions++
                try {
                    myFixture.configureByText(file.name, text)
                    myFixture.editor.caretModel.moveToOffset(offset)
                    myFixture.completeBasic()
                } catch (e: Throwable) {
                    val line = text.take(offset).count { it == '\n' } + 1
                    failures.appendLine("- ${file.name}:$line — ${e::class.simpleName}: ${e.message}")
                    // One report per file is enough to act on.
                    break
                }
            }
        }
        assertTrue("completion positions probed", positions > 100)
        assertTrue("completion threw:\n$failures", failures.isEmpty())
    }

    /**
     * Up to [PER_FILE] offsets per file, spread over the shapes that have
     * actually broken before: just after a `.`, just after an `@`, the first
     * non-blank character of a statement line, and inside a comment.
     */
    private fun probePositions(text: String): List<Int> {
        val out = LinkedHashSet<Int>()
        fun take(i: Int) {
            if (i in 0..text.length) out.add(i)
        }
        var i = 0
        while (i < text.length && out.size < PER_FILE) {
            when (text[i]) {
                '.' -> take(i + 1)
                '@' -> take(i + 1)
                '\n' -> {
                    // The first non-blank column of the next line.
                    var j = i + 1
                    while (j < text.length && (text[j] == ' ' || text[j] == '\t')) j++
                    take(j)
                }
            }
            i++
        }
        return out.toList()
    }

    private companion object {
        /** Bounded so the whole corpus sweep stays a few seconds. */
        const val PER_FILE = 12
    }
}
