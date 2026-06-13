package dev.jux.intellij.highlight

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.process.CapturingProcessHandler
import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.ExternalAnnotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.editor.Document
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiDocumentManager
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
        val root: String,
        val juxc: String,
    )

    override fun collectInformation(file: PsiFile, editor: Editor, hasErrors: Boolean): Request? {
        if (file !is JuxFile) return null
        if (ApplicationManager.getApplication().isUnitTestMode) return null
        if (JuxLspState.isServing(file.project)) return null
        // juxc reads the file from DISK; its byte offsets are relative to the
        // saved bytes. If the document has unsaved edits, those offsets wouldn't
        // line up with the in-memory text we paint into — so we'd underline the
        // wrong spans. Skip until saved (IDE autosave / Ctrl+S re-triggers the
        // pass); never show mis-positioned diagnostics. (A future juxc stdin/
        // overlay mode could make this keystroke-live.)
        if (FileDocumentManager.getInstance().isDocumentUnsaved(editor.document)) return null
        val vf = file.virtualFile ?: return null
        val juxc = JuxToolchain.find("juxc") ?: return null
        val root = checkRoot(vf.path) ?: return null
        return Request(file.project, vf.path, root, juxc)
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
        // The document gives line-start offsets for the line/column mapping —
        // line endings differ (juxc sees `\r\n` on disk; the document is
        // `\n`-normalized), so byte offsets would drift; line/column don't.
        val document = PsiDocumentManager.getInstance(file.project).getDocument(file)
        for (d in annotationResult) {
            val range = rangeFor(document, text, d) ?: continue
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
     * The document [TextRange] for a diagnostic — preferring its 1-based
     * line/column (line-ending-agnostic, so it lands correctly on the
     * `\n`-normalized document even though juxc counted `\r\n` on disk), and
     * falling back to the byte span only when line/column are absent. Clamped
     * to the current text; an empty span is widened by one char so it shows.
     */
    private fun rangeFor(document: Document?, text: String, d: JuxDiagnostic): TextRange? {
        if (document != null && d.lineStart >= 1) {
            val start = lineColOffset(document, d.lineStart, d.colStart)
            var end = lineColOffset(document, d.lineEnd.takeIf { it >= 1 } ?: d.lineStart, d.colEnd)
            if (end <= start) end = (start + 1).coerceAtMost(document.textLength)
            return if (end > start) TextRange(start, end) else null
        }
        // Fallback: UTF-8 byte mapping (correct when there are no `\r`s).
        val start = JuxCheckParser.byteToCharOffset(text, d.byteStart)
        if (start > text.length) return null
        var end = JuxCheckParser.byteToCharOffset(text, d.byteEnd)
        if (end <= start) end = (start + 1).coerceAtMost(text.length)
        return if (end > start) TextRange(start, end) else null
    }

    /** Document offset for a 1-based [line]/[col], clamped within that line. */
    private fun lineColOffset(document: Document, line: Int, col: Int): Int {
        val lineIdx = (line - 1).coerceIn(0, (document.lineCount - 1).coerceAtLeast(0))
        val lineStart = document.getLineStartOffset(lineIdx)
        val lineEnd = document.getLineEndOffset(lineIdx)
        return (lineStart + (col - 1).coerceAtLeast(0)).coerceIn(lineStart, lineEnd)
    }

    private companion object {
        const val TIMEOUT_MS = 15_000
    }
}
