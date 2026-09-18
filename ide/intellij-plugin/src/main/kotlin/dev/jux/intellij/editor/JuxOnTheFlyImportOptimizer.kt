package dev.jux.intellij.editor

import com.intellij.codeInsight.daemon.DaemonCodeAnalyzer
import com.intellij.codeInsight.daemon.impl.DaemonCodeAnalyzerEx
import com.intellij.codeInsight.daemon.impl.DaemonListeners
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.application.ModalityState
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.command.undo.UndoManager
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileEditor
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiEditorUtil
import com.intellij.util.ThreeState
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.settings.JuxCodeInsightWorkspaceSettings

/**
 * "Optimize imports on the fly" for Jux: once highlighting of a file has
 * finished, remove the imports Optimize Imports would remove.
 *
 * Modelled on how the Kotlin and Java plugins do it, including their reasons
 * for NOT doing it at a given moment:
 *
 *  - the option is off (it is off by default, as in Java);
 *  - the caret is inside the imports -- the user is typing one;
 *  - an undo or redo is in progress;
 *  - highlighting has not finished, or the file has an error outside its
 *    imports (an unresolved name may be the one an "unused" import was for);
 *  - the platform says the file cannot be changed silently (not in the
 *    project, read-only, or vetoed).
 */
object JuxOnTheFlyImportOptimizer {

    /** Called from the unused-import inspection when something would change. */
    fun schedule(file: JuxFile) {
        val project = file.project
        if (!JuxCodeInsightWorkspaceSettings.getInstance(project).optimizeImportsOnTheFly) return
        if (ApplicationManager.getApplication().isUnitTestMode) return
        val disposable = Disposer.newDisposable()
        project.messageBus.connect(disposable).subscribe(
            DaemonCodeAnalyzer.DAEMON_EVENT_TOPIC,
            object : DaemonCodeAnalyzer.DaemonListener {
                override fun daemonCancelEventOccurred(reason: String) {
                    Disposer.dispose(disposable)
                }

                override fun daemonFinished(fileEditors: Collection<FileEditor>) {
                    Disposer.dispose(disposable)
                    ApplicationManager.getApplication().invokeLater(
                        {
                            val editor = PsiEditorUtil.findEditor(file) ?: return@invokeLater
                            if (isTimeToOptimize(file, editor)) optimize(file)
                        },
                        ModalityState.nonModal(),
                        project.disposed,
                    )
                }
            },
        )
    }

    /** Whether optimizing [file] now would not get in the user's way. */
    fun isTimeToOptimize(file: PsiFile, editor: Editor): Boolean {
        val project = file.project
        if (project.isDisposed || !file.isValid || !file.isWritable || editor.isDisposed) return false
        val undo = UndoManager.getInstance(project)
        if (undo.isUndoInProgress || undo.isRedoInProgress) return false

        val imports = JuxImportSupport.collectImports(file)
        if (imports.isEmpty()) return false
        val importsRange = TextRange(
            imports.first().element.textRange.startOffset,
            imports.last().element.textRange.endOffset,
        )
        // The caret on the import lines, or right after the last one, means an
        // import is being written.
        if (editor.caretModel.offset in importsRange.startOffset..importsRange.endOffset + 1) return false

        val daemon = DaemonCodeAnalyzerEx.getInstanceEx(project)
        if (!daemon.isHighlightingAvailable(file) || !daemon.isErrorAnalyzingFinished(file)) return false
        var errorOutsideImports = false
        DaemonCodeAnalyzerEx.processHighlights(
            editor.document,
            project,
            HighlightSeverity.ERROR,
            0,
            editor.document.textLength,
        ) { info ->
            if (!importsRange.containsRange(info.startOffset, info.endOffset)) {
                errorOutsideImports = true
                false
            } else {
                true
            }
        }
        if (errorOutsideImports) return false
        return DaemonListeners.canChangeFileSilently(file, true, ThreeState.UNSURE)
    }

    /** Run Optimize Imports as one undoable command. */
    fun optimize(file: PsiFile) {
        val project = file.project
        PsiDocumentManager.getInstance(project).commitAllDocuments()
        val change = JuxImportOptimizer().processFile(file)
        WriteCommandAction.runWriteCommandAction(project, "Optimize Imports", null, change, file)
    }
}
