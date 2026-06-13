package dev.jux.intellij.run

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.SettingsEditor
import com.intellij.openapi.ui.ComboBox
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import javax.swing.JComponent
import javax.swing.JPanel

/**
 * The settings panel for a [JuxRunConfiguration]: mode (Run / Test), the `.jux`
 * file to run (test mode uses it only to find the `jux.toml` root), an optional
 * explicit `juxc` path, and the test-mode pattern/`--release` fields (§TS.8).
 */
class JuxSettingsEditor : SettingsEditor<JuxRunConfiguration>() {
    private val modeField = ComboBox(arrayOf("Run", "Test"))
    private val fileField = TextFieldWithBrowseButton()
    private val juxcField = JBTextField()
    private val patternField = JBTextField()
    private val releaseField = JBCheckBox("Release build (--release)")
    private val panel: JPanel

    init {
        // TextBrowseFolderListener form: present on every platform since 242 —
        // the 2-arg addBrowseFolderListener(Project, descriptor) overload only
        // exists on newer builds (verifier flags NoSuchMethodError on 2024.2).
        fileField.addBrowseFolderListener(
            com.intellij.openapi.ui.TextBrowseFolderListener(
                FileChooserDescriptorFactory.createSingleFileDescriptor("jux")
                    .withTitle("Select Jux File")
                    .withDescription("Choose the .jux file to run"),
                null,
            ),
        )
        juxcField.emptyText.text = "auto ( \$JUX_HOME / PATH )"
        patternField.emptyText.text = "all tests (substring filter)"
        panel = FormBuilder.createFormBuilder()
            .addLabeledComponent("Mode:", modeField)
            .addLabeledComponent("Jux file:", fileField)
            .addLabeledComponent("juxc path:", juxcField)
            .addLabeledComponent("Test pattern:", patternField)
            .addComponent(releaseField)
            .panel
        modeField.addActionListener { updateTestFieldsEnabled() }
    }

    /** Pattern + release only apply to `jux test` — grey them out in run mode. */
    private fun updateTestFieldsEnabled() {
        val test = modeField.selectedIndex == 1
        patternField.isEnabled = test
        releaseField.isEnabled = test
    }

    override fun resetEditorFrom(config: JuxRunConfiguration) {
        modeField.selectedIndex = if (config.isTestMode()) 1 else 0
        fileField.text = config.filePath
        // Show blank when it's the implicit default so the placeholder shows.
        juxcField.text = if (config.juxcPath == "juxc") "" else config.juxcPath
        patternField.text = config.testPattern
        releaseField.isSelected = config.release
        updateTestFieldsEnabled()
    }

    override fun applyEditorTo(config: JuxRunConfiguration) {
        config.mode =
            if (modeField.selectedIndex == 1) JuxRunConfiguration.MODE_TEST else JuxRunConfiguration.MODE_RUN
        config.filePath = fileField.text.trim()
        val juxc = juxcField.text.trim()
        config.juxcPath = juxc.ifBlank { "juxc" }
        config.testPattern = patternField.text.trim()
        config.release = releaseField.isSelected
    }

    override fun createEditor(): JComponent = panel
}
