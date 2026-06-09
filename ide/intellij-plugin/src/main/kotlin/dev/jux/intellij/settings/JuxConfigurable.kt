package dev.jux.intellij.settings

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.Configurable
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.ui.components.JBLabel
import com.intellij.util.ui.FormBuilder
import com.intellij.util.ui.JBUI
import dev.jux.intellij.run.JuxToolchain
import javax.swing.JButton
import javax.swing.JComponent
import javax.swing.event.DocumentEvent
import javax.swing.event.DocumentListener

/**
 * **Settings | Tools | Jux Toolchain** — the single place to point the IDE at
 * the Jux command-line tools. Empty = auto-detect (`$JUX_HOME`, `PATH`, the
 * usual install locations). The status line shows live what the current input
 * resolves to, so the user can confirm `juxc` / `juxc-lsp` were found.
 */
class JuxConfigurable : Configurable {
    private val field = TextFieldWithBrowseButton()
    private val status = JBLabel()

    override fun getDisplayName(): String = "Jux Toolchain"

    override fun createComponent(): JComponent {
        field.addBrowseFolderListener(
            "Jux Toolchain",
            "Select the Jux install root (containing bin/juxc) or the juxc executable",
            null,
            FileChooserDescriptorFactory.createSingleFileOrFolderDescriptor(),
        )
        field.textField.document.addDocumentListener(object : DocumentListener {
            override fun insertUpdate(e: DocumentEvent) = refresh()
            override fun removeUpdate(e: DocumentEvent) = refresh()
            override fun changedUpdate(e: DocumentEvent) = refresh()
        })
        val autoBtn = JButton("Auto-Detect").apply {
            addActionListener {
                JuxToolchain.autoDetectHome()?.let { field.text = it }
                refresh()
            }
        }
        status.border = JBUI.Borders.emptyTop(6)
        refresh()
        return FormBuilder.createFormBuilder()
            .addLabeledComponent("Toolchain location:", field)
            .addComponent(autoBtn)
            .addComponent(JBLabel("Leave empty to auto-detect from \$JUX_HOME, PATH, and standard install locations.").apply {
                foreground = JBUI.CurrentTheme.ContextHelp.FOREGROUND
            })
            .addComponent(status)
            .panel
    }

    /** Live "found / not found" feedback for the current field text. */
    private fun refresh() {
        val home = field.text.trim()
        val juxc = JuxToolchain.findPreview("juxc", home)
        val lsp = JuxToolchain.findPreview("juxc-lsp", home)
        status.text = when {
            juxc == null -> "<html><font color='#C75450'>juxc not found</font> — set a path, or install it on PATH / \$JUX_HOME.</html>"
            lsp == null -> "<html><font color='#499C54'>juxc: $juxc</font><br><font color='#C75450'>juxc-lsp not found (editor features need it)</font></html>"
            else -> "<html><font color='#499C54'>juxc: $juxc<br>juxc-lsp: $lsp</font></html>"
        }
    }

    override fun isModified(): Boolean =
        field.text.trim() != JuxSettings.getInstance().toolchainHome

    override fun apply() {
        JuxSettings.getInstance().toolchainHome = field.text.trim()
    }

    override fun reset() {
        field.text = JuxSettings.getInstance().toolchainHome
        refresh()
    }
}
