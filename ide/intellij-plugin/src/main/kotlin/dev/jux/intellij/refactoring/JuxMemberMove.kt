package dev.jux.intellij.refactoring

import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.DialogWrapper
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.refactoring.RefactoringActionHandler
import com.intellij.ui.components.JBCheckBox
import com.intellij.ui.components.JBLabel
import com.intellij.util.ui.FormBuilder
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxSubtypes
import dev.jux.intellij.resolve.JuxTypeIndex
import javax.swing.JComponent

/**
 * Pull Members Up and Push Members Down: fields and methods moving between a
 * class and its superclass, Java's two hierarchy refactorings.
 *
 * Each chosen member either moves, or (for a method) is made abstract: pulled
 * up as an `abstract` declaration with the body staying below under
 * `@Override`, or pushed down leaving an `abstract` declaration above. A class
 * gaining an abstract method is made `abstract`.
 *
 * Refused, with the members named, when what moves would still need what
 * stays: a pulled-up method reading a field of the subclass that is not
 * pulled with it, or a pushed-down member the superclass still uses. `private`
 * becomes `protected` on the way, since the other class has to reach it.
 */
internal object JuxMemberMove {

    /** One chosen member and whether it moves as an abstract declaration only. */
    data class Choice(val member: PsiElement, val makeAbstract: Boolean = false)

    val MEMBER_KINDS = setOf(E.FIELD_DECLARATION, E.PROPERTY_DECLARATION, E.METHOD_DECLARATION)

    /** The fields and methods declared directly in [type]. */
    fun members(type: JuxTypeDeclaration): List<PsiElement> =
        type.node.findChildByType(E.CLASS_BODY)?.psi?.children?.filter { it.elementType in MEMBER_KINDS } ?: emptyList()

    /** The superclass of [type] (an `extends` target), resolved in the project. */
    fun superclass(type: JuxTypeDeclaration): JuxTypeDeclaration? {
        val ref = JuxHierarchy.supertypeReferences(type).firstOrNull { it.second }?.first
            ?: JuxHierarchy.supertypeReferences(type).firstOrNull()?.first
            ?: return null
        return JuxTypeIndex.findType(type, JuxHierarchy.bareTypeName(ref))
    }

    // ---- pull up ---------------------------------------------------------------

    fun pullUp(type: JuxTypeDeclaration, target: JuxTypeDeclaration, choices: List<Choice>): List<JuxRefactoringUtil.Edit> {
        val moved = choices.filter { !it.makeAbstract }.map { it.member }.toSet()
        // What moves may only use members that move too (or the target's own).
        val stay = members(type).filter { it !in moved }.toSet()
        val conflicts = choices.filter { !it.makeAbstract }.flatMap { c -> usedMembers(c.member).filter { it in stay } }
            .mapNotNull { JuxRefactoringUtil.nameOf(it) }.distinct()
        if (conflicts.isNotEmpty()) {
            throw JuxExtractMethodHandler.Refusal(
                "What moves up uses ${conflicts.joinToString(", ") { "`$it`" }} of `${type.name}`, which stays. Pull those up too.",
            )
        }
        val targetBody = target.node.findChildByType(E.CLASS_BODY)?.psi ?: throw JuxExtractMethodHandler.Refusal("`${target.name}` has no body.")
        val isInterface = JuxHierarchy.isInterface(target)
        val edits = ArrayList<JuxRefactoringUtil.Edit>()
        val fields = StringBuilder()
        val methods = StringBuilder()
        var needsAbstract = false
        for (c in choices) {
            val m = c.member
            val isMethod = m.elementType === E.METHOD_DECLARATION
            if (!isMethod && isInterface) throw JuxExtractMethodHandler.Refusal("An interface holds no fields: `${JuxRefactoringUtil.nameOf(m)}` cannot move to `${target.name}`.")
            if (isMethod && (c.makeAbstract || isInterface)) {
                // The declaration goes up, the body stays here and now overrides it.
                val decl = abstractDeclaration(m, inInterface = isInterface)
                methods.append("\n\n").append(decl)
                if (!isInterface) needsAbstract = true
                if (!hasOverride(m)) edits += JuxRefactoringUtil.Edit(m.containingFile, TextRange.from(m.textRange.startOffset, 0), "@Override\n")
                existingAbstract(target, m)?.let { edits += JuxRefactoringUtil.Edit(it.containingFile, JuxRefactoringUtil.memberDeletionRange(it), "") }
                continue
            }
            val text = movedText(m, dropOverride = isMethod && JuxHierarchy.findSuperMethod(target, JuxRefactoringUtil.nameOf(m) ?: "", JuxHierarchy.arity(m)) == null)
            if (isMethod) {
                // An abstract declaration of the same method above is replaced by the body.
                existingAbstract(target, m)?.let { edits += JuxRefactoringUtil.Edit(it.containingFile, JuxRefactoringUtil.memberDeletionRange(it), "") }
                methods.append("\n\n").append(text)
            } else {
                fields.append(text).append('\n')
            }
            edits += JuxRefactoringUtil.Edit(m.containingFile, JuxRefactoringUtil.memberDeletionRange(m), "")
        }
        edits += insertions(targetBody, fields.toString(), methods.toString())
        if (needsAbstract && !JuxHierarchy.hasModifier(target, "abstract")) edits += makeAbstract(target)
        return edits
    }

    // ---- push down -------------------------------------------------------------

    fun pushDown(type: JuxTypeDeclaration, choices: List<Choice>): List<JuxRefactoringUtil.Edit> {
        val subclasses = JuxSubtypes.directSubtypes(type, JuxSubtypes.buildIndex(type.project))
        if (subclasses.isEmpty()) throw JuxExtractMethodHandler.Refusal("`${type.name}` has no subclasses to push members down to.")
        // The superclass may not keep using what leaves it.
        val leaving = choices.filter { !it.makeAbstract }.map { it.member }.toSet()
        val staying = members(type).filter { it !in leaving }
        val conflicts = staying.flatMap { usedMembers(it) }.filter { it in leaving }.mapNotNull { JuxRefactoringUtil.nameOf(it) }.distinct()
        if (conflicts.isNotEmpty()) {
            throw JuxExtractMethodHandler.Refusal(
                "`${type.name}` still uses ${conflicts.joinToString(", ") { "`$it`" }}. Keep them, or push them down as abstract.",
            )
        }
        val edits = ArrayList<JuxRefactoringUtil.Edit>()
        for (sub in subclasses) {
            val body = sub.node.findChildByType(E.CLASS_BODY)?.psi ?: continue
            val fields = StringBuilder()
            val methods = StringBuilder()
            for (c in choices) {
                val m = c.member
                val name = JuxRefactoringUtil.nameOf(m)
                val isMethod = m.elementType === E.METHOD_DECLARATION
                val existing = members(sub).firstOrNull {
                    JuxRefactoringUtil.nameOf(it) == name && (!isMethod || (it.elementType === E.METHOD_DECLARATION && JuxHierarchy.arity(it) == JuxHierarchy.arity(m)))
                }
                if (existing != null) {
                    if (!isMethod) throw JuxExtractMethodHandler.Refusal("`${sub.name}` already declares `$name`.")
                    continue // it already overrides the method
                }
                if (isMethod && JuxHierarchy.isAbstractMethod(m)) {
                    // An abstract method pushed down has no body to give; each subclass must already have one.
                    throw JuxExtractMethodHandler.Refusal("`$name` is abstract and `${sub.name}` does not implement it.")
                }
                val text = if (isMethod && c.makeAbstract) withOverride(movedText(m, dropOverride = true)) else movedText(m, dropOverride = false)
                if (isMethod) methods.append("\n\n").append(text) else fields.append(text).append('\n')
            }
            edits += insertions(body, fields.toString(), methods.toString())
        }
        for (c in choices) {
            val m = c.member
            if (c.makeAbstract && m.elementType === E.METHOD_DECLARATION) {
                edits += JuxRefactoringUtil.Edit(m.containingFile, m.textRange, abstractDeclaration(m, inInterface = JuxHierarchy.isInterface(type)))
                if (!JuxHierarchy.isInterface(type) && !JuxHierarchy.hasModifier(type, "abstract")) edits += makeAbstract(type)
            } else {
                edits += JuxRefactoringUtil.Edit(m.containingFile, JuxRefactoringUtil.memberDeletionRange(m), "")
            }
        }
        return edits
    }

    // ---- helpers -----------------------------------------------------------------

    /** Members of the same type [member] reaches: bare names and `this.x`. */
    private fun usedMembers(member: PsiElement): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        val refs = PsiTreeUtil.collectElements(member) { it.elementType === E.REFERENCE_EXPRESSION || it.elementType === E.FIELD_ACCESS_EXPRESSION }
        for (ref in refs) {
            if (ref.elementType === E.FIELD_ACCESS_EXPRESSION && ref.firstChild?.elementType !== E.THIS_EXPRESSION) continue
            JuxRefactoringUtil.resolve(ref)?.takeIf { it.elementType in MEMBER_KINDS && it !== member }?.let(out::add)
        }
        // `${wheels}` in an interpolated string reads the member too, and has
        // no reference of its own: a name no local or parameter shadows there.
        val owner = JuxHierarchy.enclosingType(member)
        if (owner != null) {
            val ownMembers = members(owner)
            val tokens = PsiTreeUtil.collectElements(member) {
                it.elementType === T.INTERP_STRING_LITERAL || it.elementType === T.INTERP_RAW_STRING_LITERAL
            }
            for (token in tokens) {
                val raw = token.elementType === T.INTERP_RAW_STRING_LITERAL
                for (name in dev.jux.intellij.editor.JuxImportSupport.interpolatedNames(token.text, raw)) {
                    if (JuxRefactoringUtil.visibleVariable(name, token) != null) continue
                    ownMembers.firstOrNull { JuxRefactoringUtil.nameOf(it) == name && it !== member }?.let(out::add)
                }
            }
        }
        return out
    }

    /** [member]'s text for its new home: dedented, `private` made `protected`, `@Override` dropped when asked. */
    private fun movedText(member: PsiElement, dropOverride: Boolean): String {
        val base = member.textRange.startOffset
        val sb = StringBuilder(member.text)
        val edits = ArrayList<Pair<TextRange, String>>()
        member.node.findChildByType(E.MODIFIER_LIST)?.findChildByType(T.PRIVATE_KW)?.let { edits += it.textRange to "protected" }
        if (dropOverride) {
            for (a in member.children.filter { it.elementType === E.ANNOTATION && it.text.trim().removePrefix("@").equals("override", true) }) {
                var end = a.textRange.endOffset
                val text = member.containingFile.text
                while (end < text.length && text[end].isWhitespace()) end++
                edits += TextRange(a.textRange.startOffset, end) to ""
            }
        }
        for ((range, text) in edits.sortedByDescending { it.first.startOffset }) {
            sb.replace(range.startOffset - base, range.endOffset - base, text)
        }
        return JuxRefactoringUtil.dedent(sb.toString())
    }

    private fun withOverride(text: String) = if (text.trimStart().startsWith("@Override", ignoreCase = true)) text else "@Override\n$text"

    private fun hasOverride(method: PsiElement) =
        method.children.any { it.elementType === E.ANNOTATION && it.text.trim().removePrefix("@").equals("override", true) }

    /** The method's signature as an `abstract` declaration (implicit in an interface). */
    private fun abstractDeclaration(method: PsiElement, inInterface: Boolean): String {
        val body = JuxRefactoringUtil.body(method) ?: return JuxRefactoringUtil.dedent(method.text)
        val start = (method.node.findChildByType(E.MODIFIER_LIST) ?: method.node.findChildByType(E.TYPE_REFERENCE))?.startOffset
            ?: method.textRange.startOffset
        var signature = method.containingFile.text.substring(start, body.textRange.startOffset).trim()
        signature = signature.replace(Regex("\\bprivate\\b"), "protected")
        if (!inInterface && !Regex("\\babstract\\b").containsMatchIn(signature)) {
            val mods = method.node.findChildByType(E.MODIFIER_LIST)
            signature = if (mods != null) signature.replaceFirst(mods.text.trim(), mods.text.trim() + " abstract") else "abstract $signature"
        }
        return "$signature;"
    }

    /** An abstract declaration in [target] of the same name and arity as [method]. */
    private fun existingAbstract(target: JuxTypeDeclaration, method: PsiElement): PsiElement? =
        members(target).firstOrNull {
            it.elementType === E.METHOD_DECLARATION && JuxRefactoringUtil.nameOf(it) == JuxRefactoringUtil.nameOf(method) &&
                JuxHierarchy.arity(it) == JuxHierarchy.arity(method) && JuxHierarchy.isAbstractMethod(it)
        }

    /** Fields after the last field, methods at the end of [body]. */
    private fun insertions(body: PsiElement, fields: String, methods: String): List<JuxRefactoringUtil.Edit> {
        val out = ArrayList<JuxRefactoringUtil.Edit>()
        val file = body.containingFile
        val members = body.children.filter { it.elementType !== T.LBRACE && it.elementType !== T.RBRACE && it !is com.intellij.psi.PsiWhiteSpace }
        val close = body.node.findChildByType(T.RBRACE)?.startOffset ?: body.textRange.endOffset
        if (fields.isNotEmpty()) {
            val lastField = members.lastOrNull { it.elementType === E.FIELD_DECLARATION || it.elementType === E.PROPERTY_DECLARATION }
            out += if (lastField != null) {
                JuxRefactoringUtil.Edit(file, TextRange.from(lastField.textRange.endOffset, 0), "\n" + fields.trimEnd())
            } else {
                val open = body.node.findChildByType(T.LBRACE)!!.textRange.endOffset
                val after = if (members.isEmpty()) "\n" else "\n\n"
                JuxRefactoringUtil.Edit(file, TextRange.from(open, 0), "\n" + fields.trimEnd() + after)
            }
        }
        if (methods.isNotEmpty()) {
            val last = members.lastOrNull()
            out += if (last != null) {
                JuxRefactoringUtil.Edit(file, TextRange.from(last.textRange.endOffset, 0), methods)
            } else {
                // An empty body: right after its `{`, keeping the line break before `}`.
                val open = body.node.findChildByType(T.LBRACE)?.textRange?.endOffset ?: close
                val tail = if (file.text.substring(open, close).contains('\n')) "" else "\n"
                JuxRefactoringUtil.Edit(file, TextRange.from(open, 0), "\n" + methods.trimStart('\n') + tail)
            }
        }
        return out
    }

    /** `abstract` added to a class declaration's modifiers. */
    private fun makeAbstract(type: JuxTypeDeclaration): JuxRefactoringUtil.Edit {
        val keyword = type.node.findChildByType(T.CLASS_KW)!!
        return JuxRefactoringUtil.Edit(type.containingFile, TextRange.from(keyword.startOffset, 0), "abstract ")
    }
}

/** A chooser of members, each with a "make abstract" box for methods. */
private class JuxMemberChooser(project: Project, title: String, members: List<PsiElement>) : DialogWrapper(project, true) {
    private val rows = members.map { m ->
        val label = if (m.elementType === E.METHOD_DECLARATION) "${JuxRefactoringUtil.nameOf(m)}(${JuxHierarchy.parameterNames(m).joinToString(", ")})"
        else JuxRefactoringUtil.nameOf(m) ?: m.text
        Triple(m, JBCheckBox(label, false), JBCheckBox("abstract", false).also { it.isEnabled = m.elementType === E.METHOD_DECLARATION })
    }

    init {
        this.title = title
        init()
    }

    override fun createCenterPanel(): JComponent {
        val form = FormBuilder.createFormBuilder().addComponent(JBLabel("Members:"))
        rows.forEach { (_, box, abstractBox) -> form.addLabeledComponent(box, abstractBox) }
        return form.panel
    }

    fun choices() = rows.filter { it.second.isSelected }.map { JuxMemberMove.Choice(it.first, it.third.isSelected) }
}

/** Shared by the two handlers: the class at the caret and the members to move. */
private abstract class JuxMemberMoveHandler(private val title: String) : RefactoringActionHandler {

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        val element = dataContext?.let { CommonDataKeys.PSI_ELEMENT.getData(it) }
            ?: file?.findElementAt(editor?.caretModel?.offset ?: return) ?: return
        val type = PsiTreeUtil.getParentOfType(element, JuxTypeDeclaration::class.java, false)
        if (type == null) {
            JuxRefactoringInput.refuse(project, editor, title, "Place the caret in a class.")
            return
        }
        val preselected = PsiTreeUtil.findFirstParent(element) { it.elementType in JuxMemberMove.MEMBER_KINDS && it.parent?.parent === type }
        run(project, editor, type, preselected)
    }

    override fun invoke(project: Project, elements: Array<out PsiElement>, dataContext: DataContext?) {
        val type = elements.firstOrNull() as? JuxTypeDeclaration ?: return
        run(project, null, type, null)
    }

    private fun run(project: Project, editor: Editor?, type: JuxTypeDeclaration, preselected: PsiElement?) {
        val members = JuxMemberMove.members(type)
        val choices = if (ApplicationManager.getApplication().isUnitTestMode) {
            val names = JuxRefactoringInput.answers[title]?.split(',')?.map { it.trim() } ?: listOfNotNull(JuxRefactoringUtil.nameOf(preselected))
            val abstracts = JuxRefactoringInput.answers["$title: abstract"]?.split(',')?.map { it.trim() } ?: emptyList()
            members.filter { JuxRefactoringUtil.nameOf(it) in names }.map { JuxMemberMove.Choice(it, JuxRefactoringUtil.nameOf(it) in abstracts) }
        } else {
            val chooser = JuxMemberChooser(project, title, members)
            if (!chooser.showAndGet()) return
            chooser.choices()
        }
        if (choices.isEmpty()) return
        val edits = try {
            plan(type, choices)
        } catch (r: JuxExtractMethodHandler.Refusal) {
            JuxRefactoringInput.refuse(project, editor, title, r.message!!)
            return
        }
        WriteCommandAction.writeCommandAction(project).withName(title).run<RuntimeException> {
            JuxRefactoringUtil.applyEdits(project, edits)
        }
    }

    abstract fun plan(type: JuxTypeDeclaration, choices: List<JuxMemberMove.Choice>): List<JuxRefactoringUtil.Edit>
}

/** Pull Members Up: to the class's superclass (or an interface it implements, as abstract declarations). */
private class JuxPullUpHandlerImpl : JuxMemberMoveHandler(JuxPullUpHandler.TITLE) {
    override fun plan(type: JuxTypeDeclaration, choices: List<JuxMemberMove.Choice>): List<JuxRefactoringUtil.Edit> {
        val target = JuxMemberMove.superclass(type)
            ?: throw JuxExtractMethodHandler.Refusal("`${type.name}` has no supertype in the project to pull members up to.")
        if (!target.isWritable || target.containingFile.virtualFile?.extension != "jux") {
            throw JuxExtractMethodHandler.Refusal("`${target.name}` is not editable project source.")
        }
        return JuxMemberMove.pullUp(type, target, choices)
    }
}

/** Push Members Down: to every direct subclass. */
private class JuxPushDownHandlerImpl : JuxMemberMoveHandler(JuxPushDownHandler.TITLE) {
    override fun plan(type: JuxTypeDeclaration, choices: List<JuxMemberMove.Choice>): List<JuxRefactoringUtil.Edit> =
        JuxMemberMove.pushDown(type, choices)
}

/** Public entry points for the refactoring support provider. */
class JuxPullUpHandler : RefactoringActionHandler by JuxPullUpHandlerImpl() {
    companion object {
        const val TITLE = "Pull Members Up"
    }
}

class JuxPushDownHandler : RefactoringActionHandler by JuxPushDownHandlerImpl() {
    companion object {
        const val TITLE = "Push Members Down"
    }
}
