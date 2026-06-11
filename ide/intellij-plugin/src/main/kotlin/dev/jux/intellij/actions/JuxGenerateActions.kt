package dev.jux.intellij.actions

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes
import dev.jux.intellij.psi.JuxFieldDeclaration
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeIndex

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
            if (hasKeyword(field, "static")) continue
            val name = field.name ?: continue
            val typeRef = field.node.findChildByType(JuxElementTypes.TYPE_REFERENCE)
            val typeText = typeRef?.text?.trim() ?: continue
            out.add(JuxField(typeText, name))
        }
        return out
    }

    private fun hasKeyword(el: PsiElement, kw: String): Boolean {
        var c: PsiElement? = el.firstChild
        while (c != null) {
            if (c.text == kw && c.elementType != JuxTokenTypes.IDENTIFIER) return true
            c = c.nextSibling
        }
        return false
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
    override fun build(className: String, fields: List<JuxField>): String? {
        if (fields.isEmpty()) return null
        return fields.joinToString("") {
            "\n    public ${it.type} ${getterName(it.name)}() {\n        return ${it.name};\n    }\n"
        }
    }
}

/** Generate a setter for each field: `public void setName(T value) { name = value; }`. */
class JuxGenerateSettersAction : JuxGenerateAction() {
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
 * supertypes (its `extends` / `implements` clauses, resolved project-wide via
 * [dev.jux.intellij.resolve.JuxTypeIndex]) that the class does not yet declare.
 * The body throws `UnsupportedOperationException` — valid for any return type —
 * so the result compiles and the user fills it in.
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

        val ownMethods = JuxHierarchy.directChildren(type, JuxElementTypes.METHOD_DECLARATION)
            .mapNotNull { (it as? dev.jux.intellij.psi.JuxNamedElement)?.name }
            .toMutableSet()
        val sb = StringBuilder()
        val seen = HashSet<String>()
        // Walk the supertype chain, breadth-ish, with a cycle/visited guard.
        val queue = ArrayDeque(JuxHierarchy.superTypeNames(type))
        val visitedTypes = HashSet<String>()
        while (queue.isNotEmpty()) {
            val superName = queue.removeFirst()
            if (!visitedTypes.add(superName)) continue
            val superDecl = JuxTypeIndex.findType(project, superName) ?: continue
            for (m in JuxHierarchy.directChildren(superDecl, JuxElementTypes.METHOD_DECLARATION)) {
                val name = (m as? dev.jux.intellij.psi.JuxNamedElement)?.name ?: continue
                if (name in ownMethods || !seen.add(name)) continue
                if (!JuxHierarchy.isOverridable(m)) continue
                val sig = JuxHierarchy.methodSignature(m) ?: continue
                sb.append("\n    @override\n    public ").append(sig)
                    .append(" {\n        throw new UnsupportedOperationException(\"TODO: ")
                    .append(name).append("\");\n    }\n")
            }
            // Climb further up this supertype's own hierarchy.
            queue.addAll(JuxHierarchy.superTypeNames(superDecl))
        }
        if (sb.isEmpty()) return
        val text = sb.toString()
        WriteCommandAction.runWriteCommandAction(project, "Override Methods", null, {
            val doc = editor.document
            val offset = editor.caretModel.offset
            doc.insertString(offset, text)
            editor.caretModel.moveToOffset(offset + text.length)
            PsiDocumentManager.getInstance(project).commitDocument(doc)
        })
    }

    private fun enclosingType(e: AnActionEvent): JuxTypeDeclaration? {
        val file = e.getData(CommonDataKeys.PSI_FILE) ?: return null
        if (file.language != JuxLanguage) return null
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return null
        val at = file.findElementAt(editor.caretModel.offset) ?: return null
        return PsiTreeUtil.getParentOfType(at, JuxTypeDeclaration::class.java)
    }
}
