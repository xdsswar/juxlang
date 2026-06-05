package dev.jux.intellij.run

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.SettingsEditor
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import javax.swing.JComponent
import javax.swing.JPanel

/**
 * The settings panel for a [JuxRunConfiguration]: the `.jux` file to run and an
 * optional explicit `juxc` path (leave blank to auto-resolve via `$JUX_HOME` /
 * `PATH`).
 */
class JuxSettingsEditor : SettingsEditor<JuxRunConfiguration>() {
    private val fileField = TextFieldWithBrowseButton()
    private val juxcField = JBTextField()
    private val panel: JPanel

    init {
        fileField.addBrowseFolderListener(
            null,
            FileChooserDescriptorFactory.createSingleFileDescriptor("jux")
                .withTitle("Select Jux File")
                .withDescription("Choose the .jux file to run"),
        )
        juxcField.emptyText.text = "auto ( \$JUX_HOME / PATH )"
        panel = FormBuilder.createFormBuilder()
            .addLabeledComponent("Jux file:", fileField)
            .addLabeledComponent("juxc path:", juxcField)
            .panel
    }

    override fun resetEditorFrom(config: JuxRunConfiguration) {
        fileField.text = config.filePath
        // Show blank when it's the implicit default so the placeholder shows.
        juxcField.text = if (config.juxcPath == "juxc") "" else config.juxcPath
    }

    override fun applyEditorTo(config: JuxRunConfiguration) {
        config.filePath = fileField.text.trim()
        val juxc = juxcField.text.trim()
        config.juxcPath = juxc.ifBlank { "juxc" }
    }

    override fun createEditor(): JComponent = panel
}
