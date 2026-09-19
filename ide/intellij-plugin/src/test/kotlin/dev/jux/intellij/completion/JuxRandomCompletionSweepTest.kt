package dev.jux.intellij.completion

import com.intellij.codeInsight.lookup.LookupManager
import com.intellij.testFramework.PlatformTestUtil
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.JuxCorpus
import java.io.File
import kotlin.random.Random

/**
 * Completion at 20 seeded-random identifier positions in every example and
 * every Jux lesson: it must never throw, and every popup must come back
 * within [BUDGET_MS].
 *
 * [JuxCorpusCompletionTest] probes the shapes that broke before (after `.`,
 * after `@`, statement starts). This one probes where a user actually is
 * most of the time: in the middle of, or at the end of, a name being typed,
 * anywhere in the file. The seed is fixed so a failure reproduces; the
 * positions still cover every file end to end rather than its first lines.
 */
class JuxRandomCompletionSweepTest : BasePlatformTestCase() {

    override fun getTestDataPath(): String = File("../../examples").absolutePath

    fun testCompletionAtRandomIdentifierPositions() {
        val examples = File(testDataPath)
        assertTrue("examples dir not found at ${examples.absolutePath}", examples.isDirectory)

        val failures = StringBuilder()
        val slow = StringBuilder()
        var positions = 0
        for ((name, file) in JuxCorpus.sweep(examples)) {
            val text = file.readText()
            // One seed per file, from its name, so each file's positions are stable.
            val random = Random(name.hashCode())
            for (offset in identifierPositions(text, random)) {
                positions++
                val line = text.take(offset).count { it == '\n' } + 1
                try {
                    myFixture.configureByText(name, text)
                    myFixture.editor.caretModel.moveToOffset(offset)
                    val start = System.nanoTime()
                    myFixture.completeBasic()
                    val ms = (System.nanoTime() - start) / 1_000_000
                    if (ms > BUDGET_MS) slow.appendLine("- ${file.relativeTo(examples)}:$line took ${ms}ms")
                } catch (e: Throwable) {
                    failures.appendLine("- ${file.relativeTo(examples)}:$line ${e::class.simpleName}: ${e.message}")
                    break
                } finally {
                    LookupManager.getInstance(project).hideActiveLookup()
                    // A real IDE runs the EDT queue between keystrokes; a tight
                    // test loop does not, and every completion's queued UI work
                    // (and the lookup it keeps alive) piles up until the heap is
                    // gone. Measured: ~2 MB per file, all platform objects.
                    PlatformTestUtil.dispatchAllEventsInIdeEventQueue()
                }
            }
        }
        assertTrue("positions probed: $positions", positions > 1000)
        assertTrue("completion threw:\n$failures", failures.isEmpty())
        assertTrue("completion over budget (${BUDGET_MS}ms):\n$slow", slow.isEmpty())
    }

    /**
     * Up to [PER_FILE] offsets inside or at the end of identifiers, picked at
     * random from all of the file's identifiers, skipping comments and strings
     * roughly (a line comment's rest, and text between double quotes).
     */
    private fun identifierPositions(text: String, random: Random): List<Int> {
        val candidates = ArrayList<Int>()
        var i = 0
        var inString = false
        while (i < text.length) {
            val c = text[i]
            when {
                !inString && c == '/' && i + 1 < text.length && text[i + 1] == '/' -> {
                    while (i < text.length && text[i] != '\n') i++
                    continue
                }
                c == '"' -> inString = !inString
                c == '\n' -> inString = false
                !inString && (c.isLetter() || c == '_') && (i == 0 || !text[i - 1].isLetterOrDigit()) -> {
                    var j = i
                    while (j < text.length && (text[j].isLetterOrDigit() || text[j] == '_')) j++
                    // Mid-name (typing) and end-of-name (just typed).
                    if (j - i >= 2) candidates.add(i + (j - i) / 2)
                    candidates.add(j)
                    i = j
                    continue
                }
            }
            i++
        }
        candidates.shuffle(random)
        return candidates.take(PER_FILE)
    }

    private companion object {
        const val PER_FILE = 20

        /** Generous for a test machine; a user feels anything much past it. */
        const val BUDGET_MS = 3_000L
    }
}
