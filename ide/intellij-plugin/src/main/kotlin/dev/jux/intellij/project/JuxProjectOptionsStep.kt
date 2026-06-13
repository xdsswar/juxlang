package dev.jux.intellij.project

import com.intellij.ide.util.projectWizard.ModuleWizardStep
import com.intellij.openapi.ui.ComboBox
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBRadioButton
import com.intellij.util.ui.FormBuilder
import com.intellij.util.ui.JBUI
import javax.swing.ButtonGroup
import javax.swing.JComponent
import javax.swing.JPanel

/**
 * The "what kind of Jux project?" wizard page: executable vs library, the
 * library crate-type, and whether to drop in starter code. Reads its result
 * back into the [JuxModuleBuilder] in [updateDataModel].
 */
class JuxProjectOptionsStep(private val builder: JuxModuleBuilder) : ModuleWizardStep() {

    private val executable = JBRadioButton(
        "Executable — a runnable program (builds a binary from main())",
        builder.projectKind == JuxProjectKind.EXECUTABLE,
    )
    private val library = JBRadioButton(
        "Library — a reusable module other Jux code depends on",
        builder.projectKind == JuxProjectKind.LIBRARY,
    )

    /** crate-type for a library (§B.2.3): Jux/Rust lib, or C-ABI shared/static. */
    private val crateType = ComboBox(arrayOf("lib", "cdylib", "dylib", "staticlib")).apply {
        selectedItem = builder.crateType
    }
    private val crateTypeLabel = JBLabel("Library type:")

    private val sample = JBCheckBox("Generate sample code", builder.generateSample)

    init {
        ButtonGroup().apply { add(executable); add(library) }
        // The crate-type choice only applies to a library.
        val sync = { setLibraryControlsEnabled(library.isSelected) }
        executable.addActionListener { sync() }
        library.addActionListener { sync() }
    }

    override fun getComponent(): JComponent {
        setLibraryControlsEnabled(library.isSelected)
        val crateRow = JPanel(java.awt.BorderLayout()).apply {
            add(crateType, java.awt.BorderLayout.CENTER)
        }
        return FormBuilder.createFormBuilder()
            .addComponent(JBLabel("What would you like to build?"))
            .addComponent(executable)
            .addComponent(library)
            .addLabeledComponent(crateTypeLabel, crateRow)
            .addComponent(sample)
            .addComponentFillVertically(JPanel(), 0)
            .panel
            .apply { border = JBUI.Borders.empty(10) }
    }

    private fun setLibraryControlsEnabled(enabled: Boolean) {
        crateType.isEnabled = enabled
        crateTypeLabel.isEnabled = enabled
    }

    override fun updateDataModel() {
        builder.projectKind = if (library.isSelected) JuxProjectKind.LIBRARY else JuxProjectKind.EXECUTABLE
        builder.crateType = (crateType.selectedItem as? String) ?: "lib"
        builder.generateSample = sample.isSelected
    }
}
