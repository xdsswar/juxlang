package dev.jux.intellij.intentions

import com.intellij.codeInsight.daemon.impl.DaemonCodeAnalyzerImpl
import com.intellij.codeInsight.intention.IntentionAction
import com.intellij.codeInsight.intention.LowPriorityAction
import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.process.CapturingProcessHandler
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.DialogWrapper
import com.intellij.psi.PsiFile
import com.intellij.ui.components.JBScrollPane
import com.intellij.ui.components.JBTextArea
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.run.JuxToolchain
import java.awt.Dimension
import javax.swing.JComponent

/**
 * Alt+Enter "Explain error E0413" on a Jux diagnostic: shows what
 * `juxc explain <code>` prints (JUX-DIAGNOSTICS-ADDENDUM §D.5.3), the
 * code's documentation from the compiler's own catalog, offline. Offered
 * whenever the caret sits on a highlight whose message carries a Jux code
 * (`[E0413] ...`, from the background `juxc --check`, the language server, or
 * a native inspection that names its code).
 */
class JuxExplainErrorIntention : IntentionAction, LowPriorityAction {

    /** The code found at the caret by the last [isAvailable]. */
    private var code: String? = null

    override fun getFamilyName(): String = "Explain Jux error"

    override fun getText(): String = code?.let { "Explain error $it" } ?: familyName

    override fun startInWriteAction(): Boolean = false

    override fun isAvailable(project: Project, editor: Editor?, file: PsiFile?): Boolean {
        if (editor == null || file?.fileType != JuxFileType) return false
        code = codeAt(project, editor)
        return code != null
    }

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?) {
        val c = code ?: editor?.let { codeAt(project, it) } ?: return
        val text = ProgressManager.getInstance().runProcessWithProgressSynchronously<String, Exception>(
            { explain(c) },
            "Explaining $c",
            true,
            project,
        )
        ExplanationDialog(project, c, text).show()
    }

    /** The first Jux code in a highlight covering the caret. */
    private fun codeAt(project: Project, editor: Editor): String? {
        val offset = editor.caretModel.offset
        val infos = DaemonCodeAnalyzerImpl.getHighlights(editor.document, HighlightSeverity.WEAK_WARNING, project)
        return infos.asSequence()
            .filter { offset >= it.startOffset && offset <= it.endOffset }
            .mapNotNull { codeIn(it.description) }
            .firstOrNull()
    }

    private class ExplanationDialog(project: Project, code: String, text: String) : DialogWrapper(project) {
        private val area = JBTextArea(text).apply {
            isEditable = false
            lineWrap = true
            wrapStyleWord = true
            font = com.intellij.util.ui.JBFont.create(java.awt.Font(java.awt.Font.MONOSPACED, java.awt.Font.PLAIN, 12))
        }

        init {
            title = "Jux $code"
            setOKButtonText("Close")
            init()
        }

        override fun createActions() = arrayOf(okAction)

        override fun createCenterPanel(): JComponent =
            JBScrollPane(area).apply { preferredSize = Dimension(640, 420) }
    }

    companion object {
        private val CODE = Regex("""\b([EW]\d{4})\b""")

        /** The Jux diagnostic code a message carries (`[E0413] no method ...` -> `E0413`). */
        fun codeIn(message: String?): String? = message?.let { CODE.find(it)?.groupValues?.get(1) }

        /** `juxc explain <code>` (or `jux explain` when only `jux` is installed). */
        fun command(code: String): List<String> = listOf("explain", code)

        /**
         * Run the explanation and return its text: stdout, else stderr (an
         * unknown code), else a hint to configure the toolchain.
         */
        fun explain(code: String): String {
            val exe = JuxToolchain.find("juxc") ?: JuxToolchain.find("jux")
                ?: return "Could not find juxc. Configure it in Settings | Tools | Jux Toolchain, or set JUX_HOME."
            return try {
                val out = CapturingProcessHandler(
                    GeneralCommandLine(exe).withParameters(command(code)).withCharset(Charsets.UTF_8),
                ).runProcess(15_000)
                out.stdout.trim().ifEmpty { out.stderr.trim() }.ifEmpty { "No explanation for $code." }
            } catch (e: Exception) {
                "Could not run ${exe.substringAfterLast('/').substringAfterLast('\\')} explain: ${e.message}"
            }
        }
    }
}
