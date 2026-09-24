package dev.jux.intellij.parser

import com.intellij.psi.PsiErrorElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.testFramework.ParsingTestCase
import dev.jux.intellij.JuxCorpus
import dev.jux.intellij.psi.JuxParserDefinition
import java.io.File

/**
 * Every generated foreign declaration stub (`*.jux.d`) in the repository must
 * parse with ZERO [PsiErrorElement]s.
 *
 * The bug this pins: the plugin refused `new` in a declaration-name slot, so
 * the BUNDLED `crates/juxc-driver/stubs/rust-std.jux.d` died at
 * `public static NonNull? new(T* ptr);` (line 1955) and every declaration after
 * it stopped being reliably indexed -- in every project, because that stub is
 * part of the toolchain. Symptom: `import rust.minifb.Window;` underlined
 * "unresolved import" while quick-doc on the same word resolved the type and
 * showed its docs.
 *
 * Deliberately corpus-shaped rather than a handful of snippets: a stub is
 * machine-written from whatever a Rust crate happens to expose, so the next
 * surprising shape will arrive in a generated file, not in a test someone
 * thought to write.
 */
class JuxStubParsingTest : ParsingTestCase("", "jux", JuxParserDefinition()) {

    fun testEveryGeneratedStubParsesWithoutErrors() {
        val repo = File(testDataPath)
        assertTrue("repo root not found at ${repo.absolutePath}", repo.isDirectory)

        val stubs = JuxCorpus.stubs(repo)
        // The bundled std stub is checked in, so it is always there. A missing
        // one means the walk broke, not that the repo is clean.
        assertTrue(
            "no `.jux.d` stubs found under ${repo.absolutePath}",
            stubs.any { it.file.name == "rust-std.jux.d" },
        )

        val failures = StringBuilder()
        for ((name, file) in stubs) {
            val psi = createFile(name, file.readText())
            val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
            if (errors.isNotEmpty()) {
                failures.appendLine("- ${file.relativeTo(repo).invariantSeparatorsPath}:")
                errors.take(8).forEach { failures.appendLine("    ${it.errorDescription} @ ${lineOf(psi, it.textOffset)}") }
            }
        }
        assertTrue("parse errors in ${stubs.size} stub files:\n$failures", failures.isEmpty())
    }

    /**
     * A stub's keyword-spelled members, inline, so the rule is readable without
     * a multi-megabyte file: `new` (every Rust constructor), `null` (a literal
     * token, `std::process::Stdio::null`), and reserved words in parameter and
     * type-alias positions.
     */
    fun testKeywordAndLiteralMemberNamesInAStub() {
        val psi = createFile(
            "demo.jux.d",
            """
            @rust("std::ptr::NonNull")
            public class NonNull<T> {
                public static NonNull? new(T* ptr);
                public static Stdio null();
                public static Stdio true();
                public bool default(int class);
            }
            """.trimIndent(),
        )
        assertNoErrors(psi)
    }

    /**
     * The same text in a hand-written `.jux` keeps the stricter recovery rule:
     * `class` is a declaration opener there, and swallowing it as a name would
     * cascade red through the rest of the type body. Only the stub relaxes.
     */
    fun testDeclarationOpenerIsStillRejectedInHandWrittenSource() {
        val psi = createPsiFile("handwritten", "public class A { public int class(); }")
        val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
        assertTrue("expected `class` in a name slot to stay an error in a .jux file", errors.isNotEmpty())
    }

    private fun assertNoErrors(psi: PsiFile) {
        val errors = PsiTreeUtil.collectElementsOfType(psi, PsiErrorElement::class.java)
        assertTrue(
            "unexpected parse errors: " + errors.joinToString { "${it.errorDescription} @ ${it.textOffset}" },
            errors.isEmpty(),
        )
    }

    /** 1-based line of [offset] in [psi], so a failure points at the stub line. */
    private fun lineOf(psi: PsiFile, offset: Int): String {
        val line = psi.text.substring(0, offset.coerceAtMost(psi.text.length)).count { it == '\n' } + 1
        return "line $line"
    }

    // The plugin module is <repo>/ide/intellij-plugin; stubs live repo-wide.
    override fun getTestDataPath(): String = File("../..").absolutePath

    override fun skipSpaces(): Boolean = false
    override fun includeRanges(): Boolean = true
}
