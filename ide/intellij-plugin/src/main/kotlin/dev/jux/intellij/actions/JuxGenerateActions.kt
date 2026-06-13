package dev.jux.intellij.actions

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.codeInsight.JuxOverrideMembers
import dev.jux.intellij.psi.JuxElementTypes
import dev.jux.intellij.psi.JuxFieldDeclaration
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/** One instance field: its declared type text and its name. */
data class JuxField(val type: String, val name: String)

/**
 * Shared base for the Alt+Insert "Generate" actions on a Jux class. Each action
 * works off the **enclosing type declaration's own fields** (no cross-file
 * resolution needed) and inserts the generated member at the caret, inside a
 * single undoable write command. Subclasses supply the member text.
 */
abstract class JuxGenerateAction : AnAction() {
    override fun getActionUpdateThread() = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        // Visible only inside a Jux class/struct/record body.
        e.presentation.isEnabledAndVisible = enclosingType(e) != null
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return
        val type = enclosingType(e) ?: return
        val className = type.name ?: return
        val fields = instanceFields(type)
        val text = build(className, fields) ?: return
        WriteCommandAction.runWriteCommandAction(project, "Generate", null, {
            val doc = editor.document
            val offset = editor.caretModel.offset
            doc.insertString(offset, text)
            editor.caretModel.moveToOffset(offset + text.length)
            PsiDocumentManager.getInstance(project).commitDocument(doc)
        })
    }

    /** Build the member text to insert, or `null` to insert nothing. */
    protected abstract fun build(className: String, fields: List<JuxField>): String?

    /**
     * Whether `{ get; set; }` properties (§P) count as fields for this action.
     * Constructor generation keeps them (settable in the constructor, §M.7.2);
     * getter/setter generation must skip them — accessors already exist.
     */
    protected open val includeProperties: Boolean = true

    private fun enclosingType(e: AnActionEvent): JuxTypeDeclaration? {
        val file = e.getData(CommonDataKeys.PSI_FILE) ?: return null
        if (file.language != JuxLanguage) return null
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return null
        val at = file.findElementAt(editor.caretModel.offset) ?: return null
        return PsiTreeUtil.getParentOfType(at, JuxTypeDeclaration::class.java)
    }

    /** Non-static fields declared directly on `type`, in source order. */
    private fun instanceFields(type: JuxTypeDeclaration): List<JuxField> {
        val out = ArrayList<JuxField>()
        for (field in PsiTreeUtil.findChildrenOfType(type, JuxFieldDeclaration::class.java)) {
            // Skip nested-type fields and statics.
            if (PsiTreeUtil.getParentOfType(field, JuxTypeDeclaration::class.java) != type) continue
            if (JuxHierarchy.hasModifier(field, "static")) continue
            if (!includeProperties && field is dev.jux.intellij.psi.JuxPropertyDeclaration) continue
            val name = field.name ?: continue
            val typeRef = field.node.findChildByType(JuxElementTypes.TYPE_REFERENCE)
            val typeText = typeRef?.text?.trim() ?: continue
            out.add(JuxField(typeText, name))
        }
        return out
    }
}

/** Generate `public ClassName(T f, …) { this.f = f; … }` from the fields. */
class JuxGenerateConstructorAction : JuxGenerateAction() {
    override fun build(className: String, fields: List<JuxField>): String? {
        val params = fields.joinToString(", ") { "${it.type} ${it.name}" }
        val assigns = fields.joinToString("\n") { "        this.${it.name} = ${it.name};" }
        val body = if (assigns.isEmpty()) "" else "\n$assigns\n    "
        return "\n    public $className($params) {$body}\n"
    }
}

/** Generate a getter for each field: `public T name() { return name; }`. */
class JuxGenerateGettersAction : JuxGenerateAction() {
    override val includeProperties: Boolean = false

    override fun build(className: String, fields: List<JuxField>): String? {
        if (fields.isEmpty()) return null
        return fields.joinToString("") {
            "\n    public ${it.type} ${getterName(it.name)}() {\n        return ${it.name};\n    }\n"
        }
    }
}

/** Generate a setter for each field: `public void setName(T value) { name = value; }`. */
class JuxGenerateSettersAction : JuxGenerateAction() {
    override val includeProperties: Boolean = false

    override fun build(className: String, fields: List<JuxField>): String? {
        if (fields.isEmpty()) return null
        return fields.joinToString("") {
            "\n    public void ${setterName(it.name)}(${it.type} value) {\n        ${it.name} = value;\n    }\n"
        }
    }
}

private fun capitalize(s: String) = if (s.isEmpty()) s else s[0].uppercaseChar() + s.substring(1)
private fun getterName(field: String) = "get${capitalize(field)}"
private fun setterName(field: String) = "set${capitalize(field)}"

/**
 * Generate `@override` stubs for the methods inherited from the type's
 * supertypes that the class does not yet declare — both kinds at once
 * (implement abstract + override concrete), through the shared
 * [JuxOverrideMembers] engine that also backs Ctrl+I / Ctrl+O.
 */
class JuxOverrideMethodsAction : AnAction() {
    override fun getActionUpdateThread() = ActionUpdateThread.BGT

    override fun update(e: AnActionEvent) {
        e.presentation.isEnabledAndVisible = enclosingType(e) != null
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return
        val type = enclosingType(e) ?: return
        JuxOverrideMembers.chooseAndInsert(
            project, editor, type,
            setOf(JuxOverrideMembers.Kind.IMPLEMENT, JuxOverrideMembers.Kind.OVERRIDE),
            "Select Methods to Override/Implement",
        )
    }

    private fun enclosingType(e: AnActionEvent): JuxTypeDeclaration? {
        val file = e.getData(CommonDataKeys.PSI_FILE) ?: return null
        if (file.language != JuxLanguage) return null
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return null
        val at = file.findElementAt(editor.caretModel.offset) ?: return null
        return PsiTreeUtil.getParentOfType(at, JuxTypeDeclaration::class.java)
    }
}
