package dev.jux.intellij.refactoring

import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.DialogWrapper
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.refactoring.RefactoringActionHandler
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBLabel
import com.intellij.ui.components.JBTextField
import com.intellij.util.ui.FormBuilder
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeEngine
import dev.jux.intellij.resolve.JuxTypeIndex
import javax.swing.JComponent

/**
 * Extract Interface and Extract Superclass, Java's two "new supertype from
 * this class" refactorings.
 *
 * - **Extract Interface** writes `interface Name { <signatures>; }` right
 *   after the class, adds `implements Name` to it, and marks each chosen
 *   method `@override`. Only instance methods can go into it; an interface
 *   method is public, so a `private` one is not offered.
 * - **Extract Superclass** moves the chosen fields and methods into a new
 *   `class Name` written right after the class, which now `extends Name`. A
 *   method chosen "abstract" stays in the class under `@override` and leaves
 *   an `abstract` declaration above (the new class becomes `abstract`).
 *   `private` becomes `protected` on the way up. When the class already
 *   extends something, the new class takes over that `extends`, as in Java.
 *
 * The new type lives in the same file, so it shares the class's package and
 * imports with nothing to copy. Refused, with the reason, when the name is
 * taken, when a moved method still needs a member that stays, or when the
 * class passes arguments to `super(...)` (the new class would need a
 * constructor this refactoring does not write).
 */
internal object JuxExtractSupertype {

    /** One chosen member, and whether it moves as an abstract declaration only. */
    data class Choice(val member: PsiElement, val makeAbstract: Boolean = false)

    /** What Extract Interface can take: the class's non-private instance methods. */
    fun interfaceCandidates(type: JuxTypeDeclaration): List<PsiElement> =
        JuxMemberMove.members(type).filter {
            it.elementType === E.METHOD_DECLARATION &&
                !JuxHierarchy.hasModifier(it, "static") &&
                !JuxHierarchy.hasModifier(it, "private")
        }

    /** What Extract Superclass can take: the class's instance fields, properties and methods. */
    fun superclassCandidates(type: JuxTypeDeclaration): List<PsiElement> =
        JuxMemberMove.members(type).filter { !JuxHierarchy.hasModifier(it, "static") }

    // ---- Extract Interface ---------------------------------------------------

    fun extractInterface(type: JuxTypeDeclaration, name: String, methods: List<PsiElement>): List<JuxRefactoringUtil.Edit> {
        checkName(type, name)
        val visibility = if (JuxHierarchy.hasModifier(type, "public")) "public " else ""
        val body = methods.joinToString("\n") { signatureOf(it) + ";" }
        val edits = ArrayList<JuxRefactoringUtil.Edit>()
        edits.add(insertAfter(type, "\n\n${visibility}interface $name {\n$body\n}"))
        edits.add(addImplements(type, name))
        methods.filterNot { hasOverride(it) }.forEach { edits.add(addOverride(it)) }
        return edits
    }

    // ---- Extract Superclass --------------------------------------------------

    fun extractSuperclass(type: JuxTypeDeclaration, name: String, choices: List<Choice>): List<JuxRefactoringUtil.Edit> {
        checkName(type, name)
        if (type.node.elementType !== E.CLASS_DECLARATION) {
            throw JuxExtractMethodHandler.Refusal("Only a class can get a superclass.")
        }
        val extends = type.node.findChildByType(E.EXTENDS_CLAUSE)?.psi
        if (extends != null && passesSuperArguments(type)) {
            throw JuxExtractMethodHandler.Refusal(
                "`${type.name}` passes arguments to `super(...)`; the new superclass would need a constructor to forward them.",
            )
        }
        val moving = choices.filterNot { it.makeAbstract }.map { it.member }.toSet()
        val staying = JuxMemberMove.members(type).filter { it !in moving }
        for (member in moving) {
            val needs = usedMembers(type, member).filter { it in staying }
            if (needs.isNotEmpty()) {
                val names = needs.mapNotNull { JuxRefactoringUtil.nameOf(it) }.joinToString(", ") { "`$it`" }
                throw JuxExtractMethodHandler.Refusal("`${JuxRefactoringUtil.nameOf(member)}` uses $names, which would stay in `${type.name}`.")
            }
        }

        val file = type.containingFile
        val abstract = choices.any { it.makeAbstract }
        val visibility = if (JuxHierarchy.hasModifier(type, "public")) "public " else ""
        val header = buildString {
            append(visibility)
            if (abstract) append("abstract ")
            append("class ").append(name)
            if (extends != null) append(' ').append(extends.text.trim())
        }
        val fields = choices.filter { it.member.elementType !== E.METHOD_DECLARATION }.map { movedText(it.member) }
        val methods = choices.filter { it.member.elementType === E.METHOD_DECLARATION }.map { c ->
            if (c.makeAbstract) "abstract " + signatureOf(c.member) + ";" else movedText(c.member)
        }
        val body = (fields + methods).joinToString("\n\n")

        val edits = ArrayList<JuxRefactoringUtil.Edit>()
        edits.add(insertAfter(type, "\n\n$header {\n$body\n}"))
        // The class now extends the new one, in place of what it extended.
        if (extends != null) {
            edits.add(JuxRefactoringUtil.Edit(file, extends.textRange, "extends $name"))
        } else {
            edits.add(insertBeforeBody(type, " extends $name"))
        }
        for (c in choices) {
            if (c.makeAbstract) {
                if (!hasOverride(c.member)) edits.add(addOverride(c.member))
            } else {
                edits.add(JuxRefactoringUtil.Edit(file, JuxRefactoringUtil.memberDeletionRange(c.member), ""))
            }
        }
        return edits
    }

    // ---- shared ----------------------------------------------------------------

    private fun checkName(type: JuxTypeDeclaration, name: String) {
        JuxRefactoringInput.identifierProblem(name)?.let { throw JuxExtractMethodHandler.Refusal(it) }
        if (JuxTypeIndex.findType(type, name) != null) {
            throw JuxExtractMethodHandler.Refusal("A type named `$name` already exists.")
        }
    }

    /**
     * A method's signature without its modifiers and body: generics, return
     * type, name, parameters and `throws`. `String describe(int width)`.
     */
    fun signatureOf(method: PsiElement): String {
        val list = method.node.findChildByType(E.MODIFIER_LIST)?.psi
        var start: PsiElement? = if (list != null) list.nextSibling else method.firstChild
        while (start is PsiWhiteSpace || start is PsiComment || start?.elementType === E.ANNOTATION) start = start?.nextSibling
        val body = method.node.findChildByType(E.CODE_BLOCK)?.psi
        val from = start?.textRange?.startOffset ?: method.textRange.startOffset
        val to = body?.textRange?.startOffset ?: method.textRange.endOffset
        return method.containingFile.text.substring(from, to).trim().removeSuffix(";").trim()
    }

    /** A member's text for its new home: `private` becomes `protected`, the rest as written. */
    private fun movedText(member: PsiElement): String {
        val text = JuxRefactoringUtil.dedent(member.text)
        return text.replaceFirst(Regex("""(^|\s)private(\s)"""), "$1protected$2")
    }

    private fun hasOverride(method: PsiElement): Boolean =
        PsiTreeUtil.collectElements(method) { it.elementType === E.ANNOTATION }
            .any { it.text.removePrefix("@").substringBefore('(').trim().equals("override", ignoreCase = true) }

    /** `@override` on its own line in front of the member (after any doc comment). */
    private fun addOverride(member: PsiElement): JuxRefactoringUtil.Edit {
        var first: PsiElement? = member.firstChild
        while (first is PsiComment || first is PsiWhiteSpace) first = first.nextSibling
        val at = (first ?: member).textRange.startOffset
        return JuxRefactoringUtil.Edit(member.containingFile, TextRange.from(at, 0), "@override\n")
    }

    /** New text right after the type declaration. */
    private fun insertAfter(type: JuxTypeDeclaration, text: String): JuxRefactoringUtil.Edit =
        JuxRefactoringUtil.Edit(type.containingFile, TextRange.from(type.textRange.endOffset, 0), text)

    /** New text between the header and the `{` of the class body. */
    private fun insertBeforeBody(type: JuxTypeDeclaration, text: String): JuxRefactoringUtil.Edit {
        val body = type.node.findChildByType(E.CLASS_BODY)?.psi
            ?: throw JuxExtractMethodHandler.Refusal("`${type.name}` has no body.")
        var last = body.prevSibling
        while (last is PsiWhiteSpace || last is PsiComment) last = last.prevSibling
        val at = (last ?: body).textRange.endOffset
        return JuxRefactoringUtil.Edit(type.containingFile, TextRange.from(at, 0), text)
    }

    /** `implements Name`, or `, Name` added to an existing implements list. */
    private fun addImplements(type: JuxTypeDeclaration, name: String): JuxRefactoringUtil.Edit {
        val clause = type.node.findChildByType(E.IMPLEMENTS_CLAUSE)?.psi
        return if (clause != null) {
            JuxRefactoringUtil.Edit(type.containingFile, TextRange.from(clause.textRange.endOffset, 0), ", $name")
        } else {
            insertBeforeBody(type, " implements $name")
        }
    }

    /** True when a constructor of [type] calls `super(...)` with arguments. */
    private fun passesSuperArguments(type: JuxTypeDeclaration): Boolean =
        PsiTreeUtil.collectElements(type) { it.elementType === E.CALL_EXPRESSION && it.text.startsWith("super(") }
            .any { !it.text.replace(" ", "").startsWith("super()") }

    /** The members of [type] that [member] reads or calls. */
    private fun usedMembers(type: JuxTypeDeclaration, member: PsiElement): List<PsiElement> {
        val out = LinkedHashSet<PsiElement>()
        val own = JuxMemberMove.members(type).toSet()
        PsiTreeUtil.processElements(member) { e ->
            val target = when (e.elementType) {
                E.REFERENCE_EXPRESSION -> runCatching { JuxTypeEngine.resolveReferenceExpression(e) }.getOrNull()
                E.FIELD_ACCESS_EXPRESSION -> runCatching { JuxTypeEngine.resolveMemberAccess(e)?.element }.getOrNull()
                else -> null
            }
            if (target != null && target in own && target !== member) out.add(target)
            true
        }
        return out.toList()
    }
}

/** Name plus members (and, for a superclass, "abstract" per method). */
private class JuxExtractSupertypeDialog(
    project: Project,
    title: String,
    defaultName: String,
    members: List<PsiElement>,
    private val offerAbstract: Boolean,
) : DialogWrapper(project, true) {
    private val nameField = JBTextField(defaultName)
    private val rows = members.map { m ->
        val label = if (m.elementType === E.METHOD_DECLARATION) JuxExtractSupertype.signatureOf(m)
        else JuxRefactoringUtil.nameOf(m) ?: m.text
        Triple(m, JBCheckBox(label, false), JBCheckBox("abstract", false).also { it.isEnabled = offerAbstract && m.elementType === E.METHOD_DECLARATION })
    }

    init {
        this.title = title
        init()
    }

    override fun createCenterPanel(): JComponent {
        val form = FormBuilder.createFormBuilder()
            .addLabeledComponent("Name:", nameField)
            .addComponent(JBLabel("Members:"))
        rows.forEach { (_, box, abstractBox) -> if (offerAbstract) form.addLabeledComponent(box, abstractBox) else form.addComponent(box) }
        return form.panel
    }

    override fun getPreferredFocusedComponent(): JComponent = nameField

    fun name(): String = nameField.text.trim()

    fun choices() = rows.filter { it.second.isSelected }.map { JuxExtractSupertype.Choice(it.first, it.third.isSelected) }
}

/** Shared by the two handlers: the class at the caret, a name, the members. */
private abstract class JuxExtractSupertypeHandler(private val title: String, private val abstractBox: Boolean) : RefactoringActionHandler {

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        val element = dataContext?.let { CommonDataKeys.PSI_ELEMENT.getData(it) }
            ?: file?.findElementAt(editor?.caretModel?.offset ?: return) ?: return
        val type = PsiTreeUtil.getParentOfType(element, JuxTypeDeclaration::class.java, false)
        if (type == null || type.node.elementType !in CLASS_LIKE) {
            JuxRefactoringInput.refuse(project, editor, title, "Place the caret in a class.")
            return
        }
        run(project, editor, type)
    }

    override fun invoke(project: Project, elements: Array<out PsiElement>, dataContext: DataContext?) {
        val type = elements.firstOrNull() as? JuxTypeDeclaration ?: return
        run(project, null, type)
    }

    private fun run(project: Project, editor: Editor?, type: JuxTypeDeclaration) {
        val candidates = candidates(type)
        val default = defaultName(type)
        val (name, choices) = if (ApplicationManager.getApplication().isUnitTestMode) {
            val name = JuxRefactoringInput.answers["$title: name"] ?: default
            val names = JuxRefactoringInput.answers["$title: members"]?.split(',')?.map { it.trim() } ?: emptyList()
            val abstracts = JuxRefactoringInput.answers["$title: abstract"]?.split(',')?.map { it.trim() } ?: emptyList()
            name to candidates.filter { JuxRefactoringUtil.nameOf(it) in names }
                .map { JuxExtractSupertype.Choice(it, JuxRefactoringUtil.nameOf(it) in abstracts) }
        } else {
            val dialog = JuxExtractSupertypeDialog(project, title, default, candidates, abstractBox)
            if (!dialog.showAndGet()) return
            dialog.name() to dialog.choices()
        }
        if (choices.isEmpty()) {
            JuxRefactoringInput.refuse(project, editor, title, "Choose at least one member.")
            return
        }
        val edits = try {
            plan(type, name, choices)
        } catch (r: JuxExtractMethodHandler.Refusal) {
            JuxRefactoringInput.refuse(project, editor, title, r.message!!)
            return
        }
        WriteCommandAction.writeCommandAction(project).withName(title).run<RuntimeException> {
            JuxRefactoringUtil.applyEdits(project, edits)
        }
    }

    abstract fun candidates(type: JuxTypeDeclaration): List<PsiElement>
    abstract fun defaultName(type: JuxTypeDeclaration): String
    abstract fun plan(type: JuxTypeDeclaration, name: String, choices: List<JuxExtractSupertype.Choice>): List<JuxRefactoringUtil.Edit>

    private companion object {
        val CLASS_LIKE = setOf(E.CLASS_DECLARATION, E.RECORD_DECLARATION, E.ENUM_DECLARATION)
    }
}

private class JuxExtractInterfaceHandlerImpl : JuxExtractSupertypeHandler(JuxExtractInterfaceHandler.TITLE, abstractBox = false) {
    override fun candidates(type: JuxTypeDeclaration) = JuxExtractSupertype.interfaceCandidates(type)
    override fun defaultName(type: JuxTypeDeclaration) = "${type.name}Contract"
    override fun plan(type: JuxTypeDeclaration, name: String, choices: List<JuxExtractSupertype.Choice>) =
        JuxExtractSupertype.extractInterface(type, name, choices.map { it.member })
}

private class JuxExtractSuperclassHandlerImpl : JuxExtractSupertypeHandler(JuxExtractSuperclassHandler.TITLE, abstractBox = true) {
    override fun candidates(type: JuxTypeDeclaration) = JuxExtractSupertype.superclassCandidates(type)
    override fun defaultName(type: JuxTypeDeclaration) = "Base${type.name}"
    override fun plan(type: JuxTypeDeclaration, name: String, choices: List<JuxExtractSupertype.Choice>) =
        JuxExtractSupertype.extractSuperclass(type, name, choices)
}

/** Public entry points for the refactoring support provider. */
class JuxExtractInterfaceHandler : RefactoringActionHandler by JuxExtractInterfaceHandlerImpl() {
    companion object {
        const val TITLE = "Extract Interface"
    }
}

class JuxExtractSuperclassHandler : RefactoringActionHandler by JuxExtractSuperclassHandlerImpl() {
    companion object {
        const val TITLE = "Extract Superclass"
    }
}
