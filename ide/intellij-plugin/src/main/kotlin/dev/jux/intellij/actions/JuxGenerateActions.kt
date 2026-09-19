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
import dev.jux.intellij.highlight.JuxTokenTypes
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
        // Visible only inside a Jux class/struct/record body, and only for the
        // kinds of type this action serves.
        val type = enclosingType(e)
        e.presentation.isEnabledAndVisible = type != null && isAvailableFor(type)
    }

    override fun actionPerformed(e: AnActionEvent) {
        val project = e.project ?: return
        val editor = e.getData(CommonDataKeys.EDITOR) ?: return
        val type = enclosingType(e) ?: return
        val className = type.name ?: return
        val offered = instanceFields(type)
        // Java's Generate asks which fields take part, all preselected.
        val fields = if (choosesFields && offered.isNotEmpty()) {
            JuxMemberChooser.chooseFields(project, className, offered, chooserTitle) ?: return
        } else {
            offered
        }
        val text = build(type, className, fields) ?: return
        WriteCommandAction.runWriteCommandAction(project, "Generate", null, {
            val doc = editor.document
            val offset = memberInsertionOffset(type, editor.caretModel.offset)
            doc.insertString(offset, text)
            editor.caretModel.moveToOffset(offset + text.length)
            PsiDocumentManager.getInstance(project).commitDocument(doc)
        })
    }

    /** Whether the user picks the fields in a chooser first, as in Java. */
    protected open val choosesFields: Boolean = false

    /** The chooser's title, when [choosesFields]. */
    protected open val chooserTitle: String = "Select Fields"

    /** Whether this action applies to [type]; every type body by default. */
    protected open fun isAvailableFor(type: JuxTypeDeclaration): Boolean = true

    /**
     * Build the member text to insert, or `null` to insert nothing. Override
     * this form when the text depends on the type itself (its kind, its type
     * parameters); the default forwards to the name-and-fields form.
     */
    protected open fun build(type: JuxTypeDeclaration, className: String, fields: List<JuxField>): String? =
        build(className, fields)

    /** Build the member text from the class name and its fields. */
    protected open fun build(className: String, fields: List<JuxField>): String? = null

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
            // A computed `Type Name -> expr;` has no storage of its own: it is
            // derived from the state, never part of it.
            if (!includeComputed && field is dev.jux.intellij.psi.JuxPropertyDeclaration &&
                field.accessorList() == null
            ) continue
            val name = field.name ?: continue
            val typeRef = field.node.findChildByType(JuxElementTypes.TYPE_REFERENCE)
            val typeText = typeRef?.text?.trim() ?: continue
            out.add(JuxField(typeText, name))
        }
        return out
    }

    /** Whether computed (`-> expr`) properties count as fields for this action. */
    protected open val includeComputed: Boolean = true
}

/**
 * Where a generated member goes. Like Java's Generate, never inside another
 * member: with the caret in a method body (or on a field) the new member lands
 * right after that member; between members it lands at the caret; outside the
 * body entirely it lands before the closing `}`.
 */
internal fun memberInsertionOffset(type: JuxTypeDeclaration, caret: Int): Int {
    val body = type.node.findChildByType(JuxElementTypes.CLASS_BODY) ?: return caret
    val open = body.findChildByType(JuxTokenTypes.LBRACE)
    val close = body.lastChildNode?.takeIf { it.elementType === JuxTokenTypes.RBRACE }
    val bodyStart = open?.textRange?.endOffset ?: body.textRange.startOffset
    val bodyEnd = close?.textRange?.startOffset ?: body.textRange.endOffset
    if (caret < bodyStart || caret > bodyEnd) return bodyEnd
    var child = body.firstChildNode
    while (child != null) {
        val range = child.textRange
        val isMember = child.elementType !== JuxTokenTypes.LBRACE &&
            child.elementType !== JuxTokenTypes.RBRACE &&
            child.psi !is com.intellij.psi.PsiWhiteSpace
        if (isMember && range.startOffset < caret && caret < range.endOffset) return range.endOffset
        child = child.treeNext
    }
    return caret
}

/**
 * The type as it is spelled in its own body: the name plus its type
 * parameters' names (`Box<T>`), which is how a parameter of the same type is
 * written. Bounds stay on the declaration.
 */
internal fun selfTypeText(type: JuxTypeDeclaration, className: String): String {
    val params = type.node.findChildByType(JuxElementTypes.TYPE_PARAMETER_LIST) ?: return className
    val names = params.getChildren(null)
        .filter { it.elementType === JuxElementTypes.TYPE_PARAMETER }
        .mapNotNull { p -> p.getChildren(null).firstOrNull { it.elementType === JuxTokenTypes.IDENTIFIER }?.text }
    return if (names.isEmpty()) className else "$className<${names.joinToString(", ")}>"
}

/** The operators [type] declares itself, by symbol or name (`==`, `hash`, `string`). */
internal fun declaredOperators(type: JuxTypeDeclaration): Set<String> {
    val body = type.node.findChildByType(JuxElementTypes.CLASS_BODY) ?: return emptySet()
    val out = HashSet<String>()
    for (member in body.getChildren(null)) {
        if (member.elementType !== JuxElementTypes.OPERATOR_DECLARATION) continue
        var leaf = member.findChildByType(JuxTokenTypes.OPERATOR_KW)?.treeNext
        while (leaf != null && leaf.psi is com.intellij.psi.PsiWhiteSpace) leaf = leaf.treeNext
        leaf?.text?.let { out.add(it) }
    }
    return out
}

/**
 * Generate `operator==` and `operator hash` together, the way Java's
 * "equals() and hashCode()" does: every field takes part in both, so equal
 * values always hash alike (§O.2.7, the pairing E0931 enforces). The hash
 * combines field hashes with the wrapping operators, since it is expected to
 * overflow. Only a class needs this: records, structs and enums derive both.
 */
class JuxGenerateEqualsAndHashAction : JuxGenerateAction() {
    override val includeComputed: Boolean = false
    override val choosesFields: Boolean = true
    override val chooserTitle: String = "Select Fields for operator== and hash"

    override fun isAvailableFor(type: JuxTypeDeclaration): Boolean =
        type.node.elementType === JuxElementTypes.CLASS_DECLARATION &&
            !declaredOperators(type).let { "==" in it && "hash" in it }

    override fun build(type: JuxTypeDeclaration, className: String, fields: List<JuxField>): String? {
        val declared = declaredOperators(type)
        val self = selfTypeText(type, className)
        val out = StringBuilder()
        if ("==" !in declared) {
            val same = if (fields.isEmpty()) "true"
            else fields.joinToString(" && ") { "${it.name} == other.${it.name}" }
            out.append("\n    public bool operator==($self other) {\n        return $same;\n    }\n")
        }
        if ("hash" !in declared) {
            out.append("\n    public int operator hash() {\n")
            if (fields.isEmpty()) {
                out.append("        return 0;\n")
            } else {
                out.append("        int result = ${fields[0].name}.operator hash();\n")
                for (field in fields.drop(1)) {
                    out.append("        result = 31 *% result +% ${field.name}.operator hash();\n")
                }
                out.append("        return result;\n")
            }
            out.append("    }\n")
        }
        return out.toString()
    }
}

/**
 * Generate `operator string`, Java's "toString()": the class name and every
 * field, `Person{name=ann, age=3}`. A record or struct already prints itself
 * but may want its own text, so it gets the action too.
 */
class JuxGenerateOperatorStringAction : JuxGenerateAction() {
    override val choosesFields: Boolean = true
    override val chooserTitle: String = "Select Fields for operator string"

    override fun isAvailableFor(type: JuxTypeDeclaration): Boolean =
        type.node.elementType in STRINGABLE && "string" !in declaredOperators(type)

    override fun build(type: JuxTypeDeclaration, className: String, fields: List<JuxField>): String {
        val parts = fields.joinToString(", ") { "${it.name}=\${${it.name}}" }
        return "\n    public String operator string() {\n        return \$\"$className{$parts}\";\n    }\n"
    }

    private companion object {
        val STRINGABLE = setOf(
            JuxElementTypes.CLASS_DECLARATION,
            JuxElementTypes.RECORD_DECLARATION,
            JuxElementTypes.STRUCT_DECLARATION,
        )
    }
}

/**
 * Generate `operator<=>`, Java's `compareTo` spelled the Jux way (§O.2.1):
 * the chosen fields compared in order, the first difference deciding. One
 * three-way comparison gives `<`, `<=`, `>` and `>=` too (§7.14.4), so this
 * is the only ordering member a type needs.
 */
class JuxGenerateOperatorCompareAction : JuxGenerateAction() {
    override val includeComputed: Boolean = false
    override val choosesFields: Boolean = true
    override val chooserTitle: String = "Select Fields for operator<=>"

    override fun isAvailableFor(type: JuxTypeDeclaration): Boolean =
        type.node.elementType in ORDERABLE && "<=>" !in declaredOperators(type)

    override fun build(type: JuxTypeDeclaration, className: String, fields: List<JuxField>): String {
        val self = selfTypeText(type, className)
        val body = StringBuilder()
        if (fields.isEmpty()) {
            body.append("        return 0;\n")
        } else {
            for (field in fields.dropLast(1)) {
                body.append("        int by${capitalize(field.name)} = ${field.name} <=> other.${field.name};\n")
                body.append("        if (by${capitalize(field.name)} != 0) {\n            return by${capitalize(field.name)};\n        }\n")
            }
            val last = fields.last().name
            body.append("        return $last <=> other.$last;\n")
        }
        return "\n    public int operator<=>($self other) {\n$body    }\n"
    }

    private companion object {
        val ORDERABLE = setOf(
            JuxElementTypes.CLASS_DECLARATION,
            JuxElementTypes.RECORD_DECLARATION,
            JuxElementTypes.STRUCT_DECLARATION,
        )
    }
}

/**
 * Generate a property for each chosen field (§P.1 form 4): `Name { get; set; }`
 * with full accessors over the field, so every read and write goes through
 * one place a setter can later validate. The property is the field's name in
 * PascalCase, the preferred spelling (W0974); a field already named that way
 * is skipped, since the two names would collide.
 */
class JuxGeneratePropertiesAction : JuxGenerateAction() {
    override val includeProperties: Boolean = false
    override val includeComputed: Boolean = false
    override val choosesFields: Boolean = true
    override val chooserTitle: String = "Select Fields to Expose as Properties"

    override fun isAvailableFor(type: JuxTypeDeclaration): Boolean =
        type.node.elementType === JuxElementTypes.CLASS_DECLARATION

    override fun build(className: String, fields: List<JuxField>): String? {
        val usable = fields.filter { capitalize(it.name) != it.name }
        if (usable.isEmpty()) return null
        return usable.joinToString("") {
            "\n    public ${it.type} ${capitalize(it.name)} {\n" +
                "        get { return this.${it.name}; }\n" +
                "        set { this.${it.name} = value; }\n" +
                "    }\n"
        }
    }
}

/** Generate `public ClassName(T f, …) { this.f = f; … }` from the fields. */
class JuxGenerateConstructorAction : JuxGenerateAction() {
    override val includeComputed: Boolean = false
    override val choosesFields: Boolean = true
    override val chooserTitle: String = "Choose Fields to Initialize by Constructor"

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
    override val choosesFields: Boolean = true
    override val chooserTitle: String = "Select Fields to Generate Getters"

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
    override val choosesFields: Boolean = true
    override val chooserTitle: String = "Select Fields to Generate Setters"

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
