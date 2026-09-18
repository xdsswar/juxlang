package dev.jux.intellij.quickfix

import com.intellij.codeInspection.LocalQuickFixAndIntentionActionOnPsiElement
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiFileFactory
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * "Create class / interface / enum / record 'Name'" on an unresolved type
 * name, Java's `CreateClassFromUsageFix` and its inner-class variant.
 *
 * A top-level type goes in a new file `Name.jux` next to the current one, in
 * the same `package` (JUX-LANG-V1 §3.1: a public type lives in the file named
 * after it). With [inner], the type is nested in the enclosing type instead.
 * Created from `new Name(a, b)`, a class gets a constructor taking those
 * arguments, and a record gets them as its components.
 */
class JuxCreateTypeFix(
    reference: PsiElement,
    private val name: String,
    private val kind: Kind,
    private val inner: Boolean,
) : LocalQuickFixAndIntentionActionOnPsiElement(reference) {

    /** The kinds of type Jux can declare from a usage. */
    enum class Kind(val keyword: String) { CLASS("class"), INTERFACE("interface"), ENUM("enum"), RECORD("record") }

    override fun getText(): String = "Create ${if (inner) "inner " else ""}${kind.keyword} '$name'"

    override fun getFamilyName(): String = "Create type from usage"

    override fun isAvailable(project: Project, file: PsiFile, startElement: PsiElement, endElement: PsiElement): Boolean {
        if (inner) return PsiTreeUtil.getParentOfType(startElement, JuxTypeDeclaration::class.java) != null
        val dir = file.originalFile.containingDirectory ?: return false
        return dir.findFile("$name.jux") == null
    }

    override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
        val newExpr = startElement.parent?.takeIf { it.elementType === E.NEW_EXPRESSION }
        val params = newExpr?.let { JuxCreateFromUsage.parametersFor(it) }.orEmpty()
        val components = params.joinToString(", ") { "${it.type ?: JuxCreateFromUsage.UNKNOWN_TYPE} ${it.name}" }
        val body = when {
            kind == Kind.CLASS && params.isNotEmpty() -> "\n    public $name($components) {\n    }\n"
            else -> "\n"
        }
        val header = if (kind == Kind.RECORD) "$name($components)" else name
        if (inner) {
            val outer = PsiTreeUtil.getParentOfType(startElement, JuxTypeDeclaration::class.java) ?: return
            val added = JuxCreateFromUsage.addMember(outer, "${kind.keyword} $header {$body}", null) ?: return
            JuxCreateFromUsage.runTemplate(project, editor, added, typeStops(added))
            return
        }
        val dir = file.originalFile.containingDirectory ?: return
        val pkg = file.node.findChildByType(E.PACKAGE_STATEMENT)?.text?.trim()
        val text = buildString {
            if (pkg != null) append(pkg).append("\n\n")
            append("public ${kind.keyword} $header {$body}\n")
        }
        val created = PsiFileFactory.getInstance(project).createFileFromText("$name.jux", JuxFileType, text)
        val added = dir.add(created) as? PsiFile ?: return
        val type = PsiTreeUtil.findChildOfType(added, JuxTypeDeclaration::class.java) ?: return
        JuxCreateFromUsage.runTemplate(project, null, type, typeStops(type))
    }

    /** The parameter / component types and names of the new type's constructor or header. */
    private fun typeStops(type: PsiElement): List<PsiElement> {
        val lists = PsiTreeUtil.collectElements(type) {
            it.elementType === E.PARAMETER_LIST || it.elementType === E.RECORD_COMPONENT_LIST
        }
        return lists.flatMap { list ->
            list.children.filter { it.elementType === E.PARAMETER || it.elementType === E.RECORD_COMPONENT }
                .flatMap { p ->
                    listOfNotNull(
                        p.node.findChildByType(E.TYPE_REFERENCE)?.psi,
                        (p as? dev.jux.intellij.psi.JuxNamedElement)?.nameIdentifier,
                    )
                }
        }
    }

    companion object {
        /**
         * The fixes for an unresolved type at [reference], by where it is
         * written: after `extends` in a class only a class fits (an interface
         * extends interfaces), after `implements` an interface, after `new` a
         * class or record, anywhere else any kind.
         */
        fun fixesFor(reference: PsiElement, name: String): List<JuxCreateTypeFix> {
            val parent = reference.parent
            val owner = parent?.parent as? JuxTypeDeclaration
            val kinds = when (parent?.elementType) {
                E.EXTENDS_CLAUSE ->
                    if (owner != null && JuxHierarchy.isInterface(owner)) listOf(Kind.INTERFACE) else listOf(Kind.CLASS)
                E.IMPLEMENTS_CLAUSE -> listOf(Kind.INTERFACE)
                E.NEW_EXPRESSION -> listOf(Kind.CLASS, Kind.RECORD)
                else -> Kind.entries
            }
            val inType = PsiTreeUtil.getParentOfType(reference, JuxTypeDeclaration::class.java) != null
            val out = kinds.map { JuxCreateTypeFix(reference, name, it, inner = false) }.toMutableList()
            // Java offers the inner variant of a class only.
            if (inType && Kind.CLASS in kinds) out.add(JuxCreateTypeFix(reference, name, Kind.CLASS, inner = true))
            return out
        }
    }
}
