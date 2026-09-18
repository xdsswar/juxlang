package dev.jux.intellij.refactoring

import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.DialogWrapper
import com.intellij.openapi.ui.ValidationInfo
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import com.intellij.refactoring.changeSignature.ChangeSignatureHandler
import com.intellij.ui.ToolbarDecorator
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBTextField
import com.intellij.ui.table.JBTable
import com.intellij.util.ui.FormBuilder
import com.intellij.util.ui.ColumnInfo
import com.intellij.util.ui.ListTableModel
import dev.jux.intellij.psi.JuxElementTypes as E
import javax.swing.JComponent

/**
 * Change Signature (`Ctrl+F6`) on a Jux method: a dialog for the new name,
 * return type and parameters, and [JuxChangeSignature] to apply them.
 *
 * Offered on a method declaration or a call of one; constructors keep their
 * type's name, so for them only the parameters change.
 */
class JuxChangeSignatureHandler : ChangeSignatureHandler {

    override fun findTargetMember(element: PsiElement): PsiElement? {
        var e: PsiElement? = element
        while (e != null && e !is PsiFile) {
            if (e.elementType in JuxRefactoringUtil.CALLABLE_KINDS) return e
            // On a call: the method it calls.
            if (e.elementType === E.CALL_EXPRESSION) {
                val callee = e.firstChild
                JuxRefactoringUtil.resolve(callee)?.takeIf { it.elementType in JuxRefactoringUtil.CALLABLE_KINDS }?.let { return it }
            }
            e = e.parent
        }
        return null
    }

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        val element = dataContext?.let { CommonDataKeys.PSI_ELEMENT.getData(it) }
            ?: file?.findElementAt(editor?.caretModel?.offset ?: return)
            ?: return
        val method = findTargetMember(element)
        if (method == null) {
            JuxRefactoringInput.refuse(project, editor, TITLE, targetNotFoundMessage)
            return
        }
        open(project, method)
    }

    override fun invoke(project: Project, elements: Array<out PsiElement>, dataContext: DataContext?) {
        val method = elements.firstNotNullOfOrNull { findTargetMember(it) } ?: return
        open(project, method)
    }

    override fun getTargetNotFoundMessage(): String = "Place the caret on a method or a call of one."

    private fun open(project: Project, method: PsiElement) {
        val dialog = JuxChangeSignatureDialog(project, method)
        if (dialog.showAndGet()) dialog.result()?.run(project)
    }

    companion object {
        const val TITLE = "Change Signature"
    }
}

/** The Change Signature dialog: name, return type, and an editable parameter table. */
private class JuxChangeSignatureDialog(project: Project, private val method: PsiElement) : DialogWrapper(project, true) {

    /** One editable row. */
    class Row(var oldIndex: Int, var name: String, var type: String, var defaultValue: String)

    private val isConstructor = method.elementType === E.CONSTRUCTOR_DECLARATION
    private val nameField = JBTextField(JuxRefactoringUtil.nameOf(method) ?: "")
    private val returnField = JBTextField(JuxChangeSignature.returnType(method) ?: "void")
    private val model = ListTableModel<Row>(
        column("Type", { it.type }) { r, v -> r.type = v },
        column("Name", { it.name }) { r, v -> r.name = v },
        column("Default value (for existing calls)", { it.defaultValue }) { r, v -> r.defaultValue = v },
    ).apply {
        items = JuxChangeSignature.current(method).map { Row(it.oldIndex, it.name, it.type, "") }
    }

    init {
        title = JuxChangeSignatureHandler.TITLE
        nameField.isEnabled = !isConstructor
        returnField.isEnabled = !isConstructor
        init()
    }

    override fun createCenterPanel(): JComponent {
        val table = JBTable(model)
        val decorated = ToolbarDecorator.createDecorator(table)
            .setAddAction { model.addRow(Row(-1, "value", "int", "0")) }
            .setRemoveAction { table.selectedRows.sortedDescending().forEach { model.removeRow(it) } }
            .setMoveUpAction { moveSelected(table, -1) }
            .setMoveDownAction { moveSelected(table, 1) }
            .createPanel()
        return FormBuilder.createFormBuilder()
            .addLabeledComponent(JBLabel("Name:"), nameField)
            .addLabeledComponent(JBLabel("Return type:"), returnField)
            .addComponentFillVertically(decorated, 4)
            .panel
    }

    private fun moveSelected(table: JBTable, delta: Int) {
        val i = table.selectedRow
        val j = i + delta
        if (i < 0 || j < 0 || j >= model.rowCount) return
        model.exchangeRows(i, j)
        table.selectionModel.setSelectionInterval(j, j)
    }

    override fun doValidate(): ValidationInfo? = result()?.problems()?.firstOrNull()?.let { ValidationInfo(it) }

    fun result(): JuxChangeSignature? = JuxChangeSignature(
        method,
        nameField.text.trim(),
        if (isConstructor) null else returnField.text.trim().ifEmpty { null },
        model.items.map { JuxChangeSignature.Parameter(it.oldIndex, it.name.trim(), it.type.trim(), it.defaultValue.trim().ifEmpty { null }) },
    )

    private companion object {
        fun column(title: String, get: (Row) -> String, set: (Row, String) -> Unit) = object : ColumnInfo<Row, String>(title) {
            override fun valueOf(item: Row): String = get(item)
            override fun isCellEditable(item: Row): Boolean = true
            override fun setValue(item: Row, value: String) = set(item, value)
        }
    }
}
