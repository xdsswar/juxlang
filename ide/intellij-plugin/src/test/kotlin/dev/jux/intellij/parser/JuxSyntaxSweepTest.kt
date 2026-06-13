package dev.jux.intellij.parser

import com.intellij.psi.PsiErrorElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.ParsingTestCase
import dev.jux.intellij.psi.JuxParserDefinition
import java.io.File

/**
 * Wide-net parser sweep beyond the `examples/` corpus: every `.jux` the
 * repo holds that real users (or the compiler team) actually feed juxc
 * must parse without a single `PsiErrorElement` — the plugin's
 * lenient-superset contract.
 *
 * Sources, each OPTIONAL (the test silently skips what's absent, so CI
 * and fresh checkouts stay green):
 *
 *  - `<repo>/probes/` — the compiler bug-hunt probes: deliberately nasty
 *    edge constructs (observer re-entrancy, async try, operator self-ref,
 *    catch binders, …). Untracked scratch files on dev machines, but the
 *    richest stress corpus available when present.
 *  - `build/syntax-sweep/stdlib/` — the embedded `jux.std` sources dumped
 *    from `crates/juxc-driver/src/stdlib_embedded.rs` (see the extractor
 *    in the repo's tooling notes). Generics-, `where`-, and `throws`-heavy
 *    real code the IDE renders through on-demand `.jux.d` stubs.
 */
class JuxSyntaxSweepTest : ParsingTestCase("", "jux", JuxParserDefinition()) {

    fun testRepoWideJuxSourcesParseWithoutErrors() {
        val roots = listOf(
            File(testDataPath, "../probes"),
            File("build/syntax-sweep/stdlib"),
        )
        val failures = StringBuilder()
        var count = 0
        for (root in roots) {
            val files = root.listFiles { _, name -> name.endsWith(".jux") } ?: continue
            for (file in files.sortedBy { it.name }) {
                count++
                val psi = createPsiFile(file.name, file.readText())
                val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
                if (errors.isNotEmpty()) {
                    failures.appendLine("• ${root.name}/${file.name}:")
                    errors.take(5).forEach {
                        failures.appendLine("    ${it.errorDescription} @ ${it.textOffset}")
                    }
                }
            }
        }
        // Nothing to sweep on this machine — fine, examples/ still gates.
        println("JuxSyntaxSweepTest: swept $count files")
        if (count == 0) return
        assertTrue("parse errors in $count swept files:\n$failures", failures.isEmpty())
    }

    // Examples live at <repo>/examples; the plugin module is <repo>/ide/intellij-plugin.
    override fun getTestDataPath(): String = File("../../examples").absolutePath

    override fun skipSpaces(): Boolean = false
    override fun includeRanges(): Boolean = true
}
