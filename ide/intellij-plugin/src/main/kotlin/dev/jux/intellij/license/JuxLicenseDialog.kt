package dev.jux.intellij.license

import com.intellij.openapi.ui.DialogWrapper
import com.intellij.ui.components.JBScrollPane
import java.awt.Dimension
import javax.swing.JComponent
import javax.swing.JTextArea

/**
 * The one-time agreement dialog: shows [JuxLicense.TEXT] in a read-only,
 * scrollable area with **I Agree** (OK) and **I Decline** (Cancel) buttons.
 * [showAndGet] returns true only when the user accepts.
 */
class JuxLicenseDialog : DialogWrapper(true) {
    init {
        title = JuxLicense.TITLE
        setOKButtonText("I Agree")
        setCancelButtonText("I Decline")
        isResizable = true
        init()
    }

    override fun createCenterPanel(): JComponent {
        val area = JTextArea(JuxLicense.TEXT).apply {
            isEditable = false
            lineWrap = true
            wrapStyleWord = true
            // Match the dialog chrome rather than the editor's code background.
            background = null
        }
        return JBScrollPane(area).apply {
            preferredSize = Dimension(600, 380)
        }
    }
}
