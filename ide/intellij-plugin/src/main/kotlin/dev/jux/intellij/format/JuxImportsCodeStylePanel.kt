package dev.jux.intellij.format

import com.intellij.application.options.CodeStyleAbstractPanel
import com.intellij.openapi.editor.colors.EditorColorsScheme
import com.intellij.openapi.editor.highlighter.EditorHighlighter
import com.intellij.openapi.fileTypes.FileType
import com.intellij.psi.codeStyle.CodeStyleSettings
import com.intellij.ui.ToolbarDecorator
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBLabel
import com.intellij.ui.table.JBTable
import com.intellij.util.ui.ColumnInfo
import com.intellij.util.ui.FormBuilder
import com.intellij.util.ui.ListTableModel
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.JuxLanguage
import javax.swing.JComponent
import javax.swing.JPanel
import javax.swing.JSpinner
import javax.swing.SpinnerNumberModel

/**
 * The Imports tab of the Jux code style page: the group layout Optimize
 * Imports lays imports out in, the blank line between groups, and the
 * wildcard threshold. The same three knobs as Java's Imports tab, over Jux's
 * import forms.
 */
class JuxImportsCodeStylePanel(settings: CodeStyleSettings) : CodeStyleAbstractPanel(JuxLanguage, null, settings) {

    /** One layout row: a group of package prefixes. */
    class Group(var prefixes: String)

    private val model = ListTableModel<Group>(object : ColumnInfo<Group, String>("Import groups, in order (package prefixes; * = all other imports)") {
        override fun valueOf(item: Group) = item.prefixes
        override fun isCellEditable(item: Group) = true
        override fun setValue(item: Group, value: String) {
            item.prefixes = value
        }
    })
    private val table = JBTable(model)
    private val blankLines = JBCheckBox("Blank line between import groups")
    private val wildcard = JSpinner(SpinnerNumberModel(0, 0, 999, 1))

    private val panel: JPanel = FormBuilder.createFormBuilder()
        .addLabeledComponent(JBLabel("Use wildcard import with this many names from one package (0 = never):"), wildcard)
        .addComponent(blankLines)
        .addComponentFillVertically(
            ToolbarDecorator.createDecorator(table)
                .setAddAction { model.addRow(Group("")) }
                .setRemoveAction { table.selectedRows.sortedDescending().forEach { model.removeRow(it) } }
                .setMoveUpAction { move(-1) }
                .setMoveDownAction { move(1) }
                .createPanel(),
            4,
        )
        .panel

    init {
        resetImpl(settings)
    }

    private fun move(delta: Int) {
        val i = table.selectedRow
        val j = i + delta
        if (i < 0 || j < 0 || j >= model.rowCount) return
        model.exchangeRows(i, j)
        table.selectionModel.setSelectionInterval(j, j)
    }

    private fun layoutText(): String =
        JuxCodeStyleSettings.formatLayout(model.items.map { g -> g.prefixes.split(',', '|').map { it.trim() }.filter { it.isNotEmpty() } }.filter { it.isNotEmpty() })

    public override fun getTabTitle(): String = "Imports"

    override fun getRightMargin(): Int = 80

    override fun createHighlighter(scheme: EditorColorsScheme): EditorHighlighter? = null

    override fun getFileType(): FileType = JuxFileType

    override fun getPreviewText(): String? = null

    override fun getPanel(): JComponent = panel

    override fun apply(settings: CodeStyleSettings) {
        val jux = settings.getCustomSettings(JuxCodeStyleSettings::class.java)
        jux.IMPORT_LAYOUT = layoutText()
        jux.BLANK_LINE_BETWEEN_IMPORT_GROUPS = blankLines.isSelected
        jux.NAMES_COUNT_TO_USE_WILDCARD = wildcard.value as Int
    }

    override fun isModified(settings: CodeStyleSettings): Boolean {
        val jux = settings.getCustomSettings(JuxCodeStyleSettings::class.java)
        return jux.IMPORT_LAYOUT != layoutText() ||
            jux.BLANK_LINE_BETWEEN_IMPORT_GROUPS != blankLines.isSelected ||
            jux.NAMES_COUNT_TO_USE_WILDCARD != wildcard.value as Int
    }

    override fun resetImpl(settings: CodeStyleSettings) {
        val jux = settings.getCustomSettings(JuxCodeStyleSettings::class.java)
        model.items = jux.layoutGroups().map { Group(it.joinToString(", ")) }
        blankLines.isSelected = jux.BLANK_LINE_BETWEEN_IMPORT_GROUPS
        wildcard.value = jux.NAMES_COUNT_TO_USE_WILDCARD
    }
}
