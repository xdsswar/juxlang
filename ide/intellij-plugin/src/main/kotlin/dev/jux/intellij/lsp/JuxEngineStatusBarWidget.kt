package dev.jux.intellij.lsp

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Disposer
import com.intellij.openapi.wm.StatusBar
import com.intellij.openapi.wm.StatusBarWidget
import com.intellij.openapi.wm.StatusBarWidgetFactory
import com.intellij.util.Alarm
import com.intellij.util.Consumer
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.run.JuxToolchain
import java.awt.event.MouseEvent

/**
 * A status-bar widget naming which engine is answering for Jux files, shown
 * only while a `.jux` file is in the editor.
 *
 * Two engines can serve semantics: `juxc-lsp` through a client, or the plugin's
 * own parser and inspections. They differ a lot — the server knows real types,
 * the fallback resolves what it can from the file and the project index — and
 * until now nothing said which one you had. When a server failed to start or
 * died, the only symptom was that everything quietly got worse, which is
 * indistinguishable from the plugin simply being bad. That is precisely the
 * class of bug the LSP4IJ liveness check ([JuxLspState.engine]) just fixed, and
 * this is how the next one gets noticed in seconds instead of a bug report.
 *
 * Clicking opens the toolchain settings, since a missing `juxc-lsp` is the
 * usual reason the fallback is in charge.
 */
class JuxEngineStatusBarWidgetFactory : StatusBarWidgetFactory {
    override fun getId(): String = WIDGET_ID

    override fun getDisplayName(): String = "Jux Language Engine"

    /** Only meaningful in a project that has Jux files; the widget hides itself otherwise. */
    override fun isAvailable(project: Project): Boolean = true

    override fun createWidget(project: Project): StatusBarWidget =
        JuxEngineStatusBarWidget(project)

    override fun canBeEnabledOn(statusBar: StatusBar): Boolean = true

    companion object {
        const val WIDGET_ID = "JuxEngineStatus"
    }
}

/** The widget itself — see [JuxEngineStatusBarWidgetFactory]. */
class JuxEngineStatusBarWidget(private val project: Project) :
    StatusBarWidget, StatusBarWidget.TextPresentation, DumbAware {

    private var statusBar: StatusBar? = null

    /**
     * The engine can change without any document edit — a server finishes
     * starting, or dies — so the text is refreshed on a slow timer rather than
     * only on editor events. `SWING_THREAD` because it touches the status bar.
     */
    private val alarm = Alarm(Alarm.ThreadToUse.SWING_THREAD, this)

    override fun ID(): String = JuxEngineStatusBarWidgetFactory.WIDGET_ID

    override fun getPresentation(): StatusBarWidget.WidgetPresentation = this

    override fun install(statusBar: StatusBar) {
        this.statusBar = statusBar
        schedule()
    }

    override fun dispose() {
        alarm.cancelAllRequests()
        statusBar = null
        Disposer.dispose(alarm)
    }

    private fun schedule() {
        if (project.isDisposed) return
        try {
            alarm.cancelAllRequests()
            alarm.addRequest({
                statusBar?.updateWidget(ID())
                schedule()
            }, REFRESH_MS)
        } catch (_: Throwable) {
            // The project may be closing concurrently; drop the request rather
            // than let an exception escape the EDT alarm.
        }
    }

    override fun getText(): String = when {
        !juxFileIsOpen() -> ""
        else -> when (engine()) {
            JuxLspState.Engine.NATIVE_LSP -> "Jux: juxc-lsp"
            JuxLspState.Engine.LSP4IJ -> "Jux: juxc-lsp (LSP4IJ)"
            JuxLspState.Engine.FALLBACK -> "Jux: no server"
        }
    }

    override fun getAlignment(): Float = java.awt.Component.CENTER_ALIGNMENT

    override fun getTooltipText(): String {
        val exe = JuxToolchain.find("juxc-lsp")
        return when (engine()) {
            JuxLspState.Engine.NATIVE_LSP, JuxLspState.Engine.LSP4IJ ->
                "Semantics come from the language server.<br>$exe"

            JuxLspState.Engine.FALLBACK -> if (exe == null) {
                "No <code>juxc-lsp</code> on PATH or \$JUX_HOME — the plugin's own " +
                    "parser and inspections are in charge.<br>Click to set the toolchain."
            } else {
                "The language server is not running — the plugin's own parser and " +
                    "inspections are in charge.<br>$exe<br>Click to set the toolchain."
            }
        }
    }

    override fun getClickConsumer(): Consumer<MouseEvent> = Consumer {
        com.intellij.openapi.options.ShowSettingsUtil.getInstance()
            .showSettingsDialog(project, "Jux Toolchain")
    }

    private fun engine(): JuxLspState.Engine = try {
        JuxLspState.engine(project)
    } catch (_: Throwable) {
        JuxLspState.Engine.FALLBACK
    }

    /** True while any open editor holds a Jux file — the widget is silent otherwise. */
    private fun juxFileIsOpen(): Boolean = try {
        if (ApplicationManager.getApplication().isUnitTestMode) {
            false
        } else {
            FileEditorManager.getInstance(project).selectedEditors
                .mapNotNull { (it as? com.intellij.openapi.fileEditor.TextEditor)?.editor }
                .any { it.holdsJuxFile() }
        }
    } catch (_: Throwable) {
        false
    }

    private fun Editor.holdsJuxFile(): Boolean =
        com.intellij.psi.PsiDocumentManager.getInstance(this@JuxEngineStatusBarWidget.project)
            .getPsiFile(document)?.fileType == JuxFileType

    private companion object {
        /** Slow enough to cost nothing, fast enough to notice a server dying. */
        const val REFRESH_MS = 3000
    }
}
