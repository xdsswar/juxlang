package dev.jux.intellij.highlight

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.process.CapturingProcessHandler
import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.ExternalAnnotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiFile
import dev.jux.intellij.diagnostics.JuxCheckParser
import dev.jux.intellij.diagnostics.JuxDiagnostic
import dev.jux.intellij.lsp.JuxLspState
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.run.JuxToolchain
import java.io.File

/**
 * Always-on semantic diagnostics for `.jux` files: runs `juxc --check
 * --diagnostic-format json` over the project on a background thread and paints
 * the compiler's real errors/warnings (unknown symbols, type mismatches, …)
 * inline — even when no language-server session is up.
 *
 * Stands down when:
 *  - `juxc-lsp` is actively serving (the LSP already publishes diagnostics —
 *    [JuxLspState.isServing]); double-painting would only duplicate them;
 *  - there's no resolvable `juxc` toolchain (degrade silently — the rest of the
 *    plugin still works);
 *  - in unit-test mode (no compiler in the fixture).
 *
 * `--check` emits NO Rust crate and never invokes `cargo`, so this is free of
 * filesystem side effects and safe to run repeatedly as the file changes.
 * IntelliJ's [ExternalAnnotator] contract debounces and runs [doAnnotate] off
 * the EDT for us.
 */
class JuxSemanticAnnotator : ExternalAnnotator<JuxSemanticAnnotator.Request, List<JuxDiagnostic>>() {

    /** Everything [doAnnotate] needs, captured on the EDT in [collectInformation]. */
    data class Request(
        val project: Project,
        val filePath: String,
        val text: String,
        val root: String,
        val juxc: String,
    )

    override fun collectInformation(file: PsiFile, editor: Editor, hasErrors: Boolean): Request? {
        if (file !is JuxFile) return null
        if (ApplicationManager.getApplication().isUnitTestMode) return null
        if (JuxLspState.isServing(file.project)) return null
        val vf = file.virtualFile ?: return null
        val juxc = JuxToolchain.find("juxc") ?: return null
        val root = checkRoot(vf.path) ?: return null
        return Request(file.project, vf.path, file.text, root, juxc)
    }

    override fun doAnnotate(info: Request): List<JuxDiagnostic> {
        val workDir = File(info.root).let { if (it.isDirectory) it else it.parentFile }
        val cmd = GeneralCommandLine(info.juxc, "--check", "--diagnostic-format", "json", info.root)
            .withWorkDirectory(workDir)
        val output = try {
            CapturingProcessHandler(cmd).runProcess(TIMEOUT_MS)
        } catch (_: Throwable) {
            return emptyList()
        }
        if (output.isTimeout || output.isCancelled) return emptyList()
        // juxc analyses the whole workspace; keep only this file's diagnostics.
        return JuxCheckParser.parse(output.stdout).filter { sameFile(it.file, info.filePath) }
    }

    override fun apply(file: PsiFile, annotationResult: List<JuxDiagnostic>, holder: AnnotationHolder) {
        val text = file.text
        for (d in annotationResult) {
            val range = byteRange(text, d.byteStart, d.byteEnd) ?: continue
            val severity = when (d.severity) {
                "warning", "lint" -> HighlightSeverity.WARNING
                "note", "help" -> HighlightSeverity.WEAK_WARNING
                else -> HighlightSeverity.ERROR
            }
            holder.newAnnotation(severity, "[${d.code}] ${d.message}").range(range).create()
        }
    }

    // ---- helpers ---------------------------------------------------------------

    /**
     * The directory to hand `juxc` so cross-file `import`s resolve: the nearest
     * ancestor holding a `jux.toml`, else the file's own directory (which still
     * gets same-package files right).
     */
    private fun checkRoot(filePath: String): String? {
        val start = File(filePath).parentFile ?: return null
        var probe: File? = start
        while (probe != null) {
            if (File(probe, "jux.toml").isFile) return probe.path
            probe = probe.parentFile
        }
        return start.path
    }

    /** Same file, comparing canonical, case-folded, forward-slashed paths. */
    private fun sameFile(a: String, b: String): Boolean {
        fun norm(p: String): String = try {
            File(p).canonicalPath
        } catch (_: Throwable) {
            p
        }.replace('\\', '/').lowercase()
        return norm(a) == norm(b)
    }

    /**
     * Convert juxc's UTF-8 byte span to a document [TextRange], clamped to the
     * current text. An empty span is widened to one char so the marker shows.
     */
    private fun byteRange(text: String, byteStart: Int, byteEnd: Int): TextRange? {
        val start = JuxCheckParser.byteToCharOffset(text, byteStart)
        if (start > text.length) return null
        var end = JuxCheckParser.byteToCharOffset(text, byteEnd)
        if (end <= start) end = (start + 1).coerceAtMost(text.length)
        return if (end > start) TextRange(start, end) else null
    }

    private companion object {
        const val TIMEOUT_MS = 15_000
    }
}
