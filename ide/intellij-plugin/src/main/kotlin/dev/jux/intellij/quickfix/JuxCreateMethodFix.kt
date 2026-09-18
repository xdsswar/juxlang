package dev.jux.intellij.quickfix

import com.intellij.codeInsight.intention.PsiElementBaseIntentionAction
import com.intellij.codeInspection.LocalQuickFixAndIntentionActionOnPsiElement
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * Builds the method a "Create method" fix adds, Java's
 * `CreateMethodFromUsageFix`: parameters from the call's arguments, the
 * return type from where the call's value goes (`void` for a bare
 * statement), and a body that throws `UnsupportedOperationException` (a Jux
 * built-in) until it is written. The result is a live template over the
 * return type and every parameter.
 */
object JuxMethodCreator {

    /** Where the new method goes. */
    sealed class Target {
        /** Into a type body; [after] is the member to follow, if in that body. */
        data class InType(val type: JuxTypeDeclaration, val isStatic: Boolean, val visibility: String?, val after: PsiElement?) : Target()

        /** A free function after [after], a top-level declaration of [file]. */
        data class TopLevel(val file: JuxFile, val after: PsiElement?) : Target()
    }

    fun create(project: Project, editor: Editor?, call: PsiElement, name: String, target: Target) {
        val params = JuxCreateFromUsage.parametersFor(call)
        val returnType =
            if (call.parent?.elementType === E.EXPRESSION_STATEMENT) "void"
            else JuxCreateFromUsage.expectedType(call) ?: JuxCreateFromUsage.UNKNOWN_TYPE
        val paramText = params.joinToString(", ") { "${it.type ?: JuxCreateFromUsage.UNKNOWN_TYPE} ${it.name}" }
        val isInterface = target is Target.InType && JuxHierarchy.isInterface(target.type)
        val modifiers = when (target) {
            is Target.InType -> listOfNotNull(
                target.visibility.takeUnless { isInterface },
                "static".takeIf { target.isStatic },
            ).joinToString(" ")
            is Target.TopLevel -> ""
        }
        val head = listOf(modifiers, returnType, "$name($paramText)").filter { it.isNotEmpty() }.joinToString(" ")
        val text =
            if (isInterface && !(target as Target.InType).isStatic) "$head;"
            else "$head {\n    ${JuxCreateFromUsage.NOT_IMPLEMENTED_BODY}\n}"
        val added: PsiElement = when (target) {
            is Target.InType -> JuxCreateFromUsage.addMember(target.type, text, target.after) ?: return
            is Target.TopLevel -> JuxCreateFromUsage.addTopLevel(target.file, text, target.after) ?: return
        }
        val stops = ArrayList<PsiElement>()
        added.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let(stops::add)
        stops.addAll(JuxCreateFromUsage.parameterStops(added.node.findChildByType(E.PARAMETER_LIST)?.psi))
        JuxCreateFromUsage.runTemplate(project, editor, added, stops)
    }

    /**
     * Where a call's method belongs when the call names no receiver: the
     * enclosing type (after the member holding the call), or next to the
     * enclosing free function.
     */
    fun targetForBareCall(call: PsiElement): Target? {
        val type = PsiTreeUtil.getParentOfType(call, JuxTypeDeclaration::class.java)
        val member = JuxCreateFromUsage.enclosingMember(call)
        if (type != null) {
            return Target.InType(type, JuxCreateFromUsage.isInStaticContext(call), "private", member)
        }
        val file = call.containingFile as? JuxFile ?: return null
        return Target.TopLevel(file, member)
    }

    /**
     * Where `receiver.name(...)` would find its method: the receiver's class
     * when it is a project type the IDE can write to and fully see (every
     * supertype resolves, so an inherited method cannot be hiding in a type
     * the IDE does not know). Null otherwise.
     */
    fun targetForMemberCall(access: PsiElement): Target? {
        val receiver = JuxTypeEngine.firstExpressionChild(access) ?: return null
        val receiverType = JuxTypeEngine.stripNullable(JuxTypeEngine.typeOf(receiver))
        val (decl, isStatic) = when (receiverType) {
            is JuxType.Static -> receiverType.decl to true
            is JuxType.ClassType -> receiverType.decl to false
            else -> return null
        }
        if (!JuxCreateFromUsage.isEditable(decl)) return null
        if (!fullyVisible(decl)) return null
        val sameType = PsiTreeUtil.getParentOfType(access, JuxTypeDeclaration::class.java) == decl
        val after = if (sameType) JuxCreateFromUsage.enclosingMember(access) else null
        return Target.InType(decl, isStatic, if (sameType) "private" else "public", after)
    }

    /** Every supertype of [decl], transitively, is a declaration the IDE can read. */
    private fun fullyVisible(decl: JuxTypeDeclaration): Boolean {
        val all = JuxTypeEngine.typeAndSupertypes(JuxTypeEngine.selfType(decl))
        return all.all { t -> JuxHierarchy.supertypeReferences(t.decl).size == JuxTypeEngine.supertypes(t).size }
    }

    /** True when the call's method name resolves to nothing. */
    fun isUnresolvedMemberCall(access: PsiElement, call: PsiElement): Boolean {
        val name = JuxTypeEngine.memberName(access) ?: return false
        val receiver = JuxTypeEngine.firstExpressionChild(access) ?: return false
        val members = JuxTypeEngine.membersOf(JuxTypeEngine.typeOf(receiver))
        return members.none { (it.element as? dev.jux.intellij.psi.JuxNamedElement)?.name == name } &&
            JuxTypeEngine.resolveMemberAccess(access, JuxTypeEngine.argumentCount(call)) == null
    }
}

/**
 * "Create method 'foo'" on an unresolved bare call `foo(a, b)`, offered by
 * the unresolved-reference inspection.
 */
class JuxCreateMethodFix(reference: PsiElement, private val name: String) :
    LocalQuickFixAndIntentionActionOnPsiElement(reference) {

    override fun getText(): String = "Create method '$name'"

    override fun getFamilyName(): String = "Create method from usage"

    override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
        val call = startElement.parent?.takeIf { it.elementType === E.CALL_EXPRESSION } ?: return
        val target = JuxMethodCreator.targetForBareCall(call) ?: return
        JuxMethodCreator.create(project, editor, call, name, target)
    }
}

/**
 * "Create method 'foo' in 'Type'" on `receiver.foo(...)` when `foo` exists
 * nowhere in the receiver's class or its supertypes. An intention rather than
 * a highlighted problem: member resolution belongs to the language server,
 * which draws the error; this adds Java's fix to it (Alt+Enter on the name).
 */
class JuxCreateMethodFromUsageIntention : PsiElementBaseIntentionAction() {

    override fun getFamilyName(): String = "Create method from usage"

    override fun isAvailable(project: Project, editor: Editor?, element: PsiElement): Boolean {
        val (access, call) = memberCallAt(element) ?: return false
        if (!JuxMethodCreator.isUnresolvedMemberCall(access, call)) return false
        val target = JuxMethodCreator.targetForMemberCall(access) as? JuxMethodCreator.Target.InType ?: return false
        text = "Create method '${JuxTypeEngine.memberName(access)}' in '${target.type.name}'"
        return true
    }

    /** The method may go into another file, which must be made writable first (outside the write action). */
    override fun startInWriteAction(): Boolean = false

    override fun invoke(project: Project, editor: Editor?, element: PsiElement) {
        val (access, call) = memberCallAt(element) ?: return
        val name = JuxTypeEngine.memberName(access) ?: return
        val target = JuxMethodCreator.targetForMemberCall(access) as? JuxMethodCreator.Target.InType ?: return
        if (!com.intellij.codeInsight.FileModificationService.getInstance().preparePsiElementForWrite(target.type)) return
        com.intellij.openapi.command.WriteCommandAction.runWriteCommandAction(project, familyName, null, {
            JuxMethodCreator.create(project, editor, call, name, target)
        })
    }

    /** The `receiver.name` and its call when the caret is on the member name of a member call. */
    private fun memberCallAt(element: PsiElement): Pair<PsiElement, PsiElement>? {
        val access = element.parent?.takeIf { it.elementType === E.FIELD_ACCESS_EXPRESSION } ?: return null
        if (JuxTypeEngine.memberName(access) != element.text) return null
        val call = access.parent?.takeIf { it.elementType === E.CALL_EXPRESSION && it.firstChild === access } ?: return null
        return access to call
    }
}
