package dev.jux.intellij.run

import com.intellij.openapi.fileChooser.FileChooserDescriptorFactory
import com.intellij.openapi.options.SettingsEditor
import com.intellij.openapi.ui.ComboBox
import com.intellij.openapi.ui.TextFieldWithBrowseButton
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import java.io.File
import javax.swing.DefaultComboBoxModel
import javax.swing.JComponent
import javax.swing.JPanel

/**
 * The settings panel for a [JuxRunConfiguration]:
 *
 *  - **Mode**: Run, Test (`jux test`), or Doc examples (`jux test --doc`).
 *  - **Jux file**: the file to run; the test modes use it only to find the
 *    project's `jux.toml`.
 *  - **Profile**: `--profile <name>` (§B.9). The picker offers the built-in
 *    profiles plus every `[profile.<name>]` of the manifest; it is editable,
 *    so a profile added a moment ago can be typed in. Blank = the default.
 *  - **Example**: `--example <name>` (§B.1.3), offered from the project's
 *    `examples/` directory, with "(main program)" and "(all examples)"
 *    (`jux build --examples`) at the top.
 *  - an optional explicit `juxc` path, and the test-only pattern and
 *    `--release` fields (§TS.8).
 *
 * The pickers are refilled from the manifest each time the editor is reset,
 * so they follow edits to `jux.toml` without a restart.
 */
class JuxSettingsEditor : SettingsEditor<JuxRunConfiguration>() {
    private val modeField = ComboBox(arrayOf(MODE_LABEL_RUN, MODE_LABEL_TEST, MODE_LABEL_DOC))
    private val fileField = TextFieldWithBrowseButton()
    private val profileField = ComboBox<String>().apply { isEditable = true }
    private val exampleField = ComboBox<String>().apply { isEditable = true }
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
            .addLabeledComponent("Profile:", profileField)
            .addLabeledComponent("Example:", exampleField)
            .addLabeledComponent("juxc path:", juxcField)
            .addLabeledComponent("Test pattern:", patternField)
            .addComponent(releaseField)
            .panel
        modeField.addActionListener { updateFieldsEnabled() }
    }

    /**
     * Pattern + release only apply to `jux test`; the example only to a run
     * (and the pattern not to doc examples, which have no filter).
     */
    private fun updateFieldsEnabled() {
        val mode = modeField.selectedIndex
        patternField.isEnabled = mode == 1
        releaseField.isEnabled = mode != 0
        exampleField.isEnabled = mode == 0
    }

    override fun resetEditorFrom(config: JuxRunConfiguration) {
        modeField.selectedIndex = when (config.mode) {
            JuxRunConfiguration.MODE_TEST -> 1
            JuxRunConfiguration.MODE_DOCTEST -> 2
            else -> 0
        }
        fileField.text = config.filePath
        // Show blank when it's the implicit default so the placeholder shows.
        juxcField.text = if (config.juxcPath == "juxc") "" else config.juxcPath
        patternField.text = config.testPattern
        releaseField.isSelected = config.release

        val root = config.manifestRoot()
        val manifest = root?.let { File(it, "jux.toml") }?.takeIf { it.isFile }
        val profiles = listOf("") + JuxManifestInfo.profiles(manifest?.readTextOrEmpty().orEmpty())
        profileField.model = DefaultComboBoxModel(profiles.toTypedArray())
        profileField.selectedItem = config.profile

        val examples = listOf(EXAMPLE_MAIN, EXAMPLE_ALL) + (root?.let(JuxManifestInfo::examples) ?: emptyList())
        exampleField.model = DefaultComboBoxModel(examples.toTypedArray())
        exampleField.selectedItem = exampleLabel(config.example)
        updateFieldsEnabled()
    }

    override fun applyEditorTo(config: JuxRunConfiguration) {
        config.mode = when (modeField.selectedIndex) {
            1 -> JuxRunConfiguration.MODE_TEST
            2 -> JuxRunConfiguration.MODE_DOCTEST
            else -> JuxRunConfiguration.MODE_RUN
        }
        config.filePath = fileField.text.trim()
        val juxc = juxcField.text.trim()
        config.juxcPath = juxc.ifBlank { "juxc" }
        config.testPattern = patternField.text.trim()
        config.release = releaseField.isSelected
        config.profile = comboText(profileField)
        config.example = exampleValue(comboText(exampleField))
    }

    override fun createEditor(): JComponent = panel

    /** The editable combo's text: what was typed, else the selected item. */
    private fun comboText(box: ComboBox<String>): String =
        ((box.editor?.item ?: box.selectedItem) as? String)?.trim().orEmpty()

    private fun File.readTextOrEmpty(): String = try {
        readText()
    } catch (_: Exception) {
        ""
    }

    companion object {
        const val MODE_LABEL_RUN = "Run"
        const val MODE_LABEL_TEST = "Test"
        const val MODE_LABEL_DOC = "Doc examples (jux test --doc)"

        /** Picker label for "no example": the package's main program. */
        const val EXAMPLE_MAIN = "(main program)"

        /** Picker label for [JuxRunCommands.ALL_EXAMPLES]. */
        const val EXAMPLE_ALL = "(all examples: jux build --examples)"

        /** Stored example value -> picker label. */
        fun exampleLabel(value: String): String = when (value.trim()) {
            "" -> EXAMPLE_MAIN
            JuxRunCommands.ALL_EXAMPLES -> EXAMPLE_ALL
            else -> value.trim()
        }

        /** Picker label (or typed text) -> stored example value. */
        fun exampleValue(label: String): String = when (label.trim()) {
            "", EXAMPLE_MAIN -> ""
            EXAMPLE_ALL -> JuxRunCommands.ALL_EXAMPLES
            else -> label.trim()
        }
    }
}
