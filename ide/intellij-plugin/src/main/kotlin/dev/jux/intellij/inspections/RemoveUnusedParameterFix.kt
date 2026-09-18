package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxParameter
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * "Remove unused parameter", Java's safe-delete of a parameter: drops the
 * parameter and the argument every caller passes for it.
 *
 * Offered only when the IDE sees every caller: the method is `private`
 * (visible in this file only, JUX-LANG-V1 §6.6), it has no overload of the
 * same name, and every mention of its name is a direct call with the full
 * argument list, never a method reference or a value that escapes. The
 * argument each call passes must also be free of side effects, since
 * removing it stops evaluating it.
 */
class RemoveUnusedParameterFix : LocalQuickFix {

    override fun getFamilyName(): String = "Remove unused parameter"

    override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
        val param = descriptor.psiElement?.parent as? JuxParameter ?: return
        val calls = callSites(param) ?: return
        val method = param.parent?.parent ?: return
        val index = JuxHierarchy.parameters(method).indexOf(param)
        for (call in calls) {
            val args = call.node.findChildByType(E.ARGUMENT_LIST)?.psi ?: continue
            JuxTypeEngine.expressionChildren(args).getOrNull(index)?.let { removeListItem(it) }
        }
        removeListItem(param)
    }

    companion object {
        /**
         * Every call to [param]'s method in its file, or null when the
         * parameter may not be removed safely (see the class doc).
         */
        fun callSites(param: JuxParameter): List<PsiElement>? {
            val method = param.parent?.parent ?: return null
            if (method.elementType !== E.METHOD_DECLARATION) return null
            if (!JuxHierarchy.hasModifier(method, "private")) return null
            val name = (method as? JuxNamedElement)?.name ?: return null
            val params = JuxHierarchy.parameters(method)
            val index = params.indexOf(param)
            if (index < 0 || params.any { it.node.findChildByType(T.ELLIPSIS) != null }) return null
            val file = method.containingFile ?: return null
            val calls = ArrayList<PsiElement>()
            var safe = true
            PsiTreeUtil.processElements(file) { e ->
                if (e.elementType === E.METHOD_DECLARATION && e !== method && (e as? JuxNamedElement)?.name == name) safe = false
                if (e.elementType === T.IDENTIFIER && e.text == name && e !== (method as JuxNamedElement).nameIdentifier) {
                    val call = callOf(e)
                    if (call == null || JuxTypeEngine.argumentCount(call) != params.size) safe = false
                    else {
                        val arg = call.node.findChildByType(E.ARGUMENT_LIST)?.psi
                            ?.let { JuxTypeEngine.expressionChildren(it).getOrNull(index) }
                        if (arg == null || !JuxCodeFacts.isSideEffectFree(arg)) safe = false else calls.add(call)
                    }
                }
                safe
            }
            return if (safe) calls else null
        }

        /** The call [nameLeaf] names the callee of (`name(...)`, `recv.name(...)`), or null. */
        private fun callOf(nameLeaf: PsiElement): PsiElement? {
            val ref = nameLeaf.parent ?: return null
            if (ref.elementType !== E.REFERENCE_EXPRESSION && ref.elementType !== E.FIELD_ACCESS_EXPRESSION) return null
            if (ref.elementType === E.FIELD_ACCESS_EXPRESSION && JuxTypeEngine.memberName(ref) != nameLeaf.text) return null
            val call = ref.parent ?: return null
            return call.takeIf { it.elementType === E.CALL_EXPRESSION && it.firstChild === ref }
        }

        /** Deletes a comma-separated list item with its comma and the space around it. */
        fun removeListItem(item: PsiElement) {
            fun skipSpace(e: PsiElement?, forward: Boolean): PsiElement? {
                var cur = e
                while (cur is PsiWhiteSpace || cur is PsiComment) cur = if (forward) cur.nextSibling else cur.prevSibling
                return cur
            }
            val next = skipSpace(item.nextSibling, forward = true)
            if (next?.elementType === T.COMMA) {
                // `a, b` → drop `a, ` : the item through the space after the comma.
                val end = skipSpace(next.nextSibling, forward = true)?.prevSibling ?: next
                item.parent.deleteChildRange(item, end)
                return
            }
            val prev = skipSpace(item.prevSibling, forward = false)
            if (prev?.elementType === T.COMMA) {
                item.parent.deleteChildRange(prev, item)
                return
            }
            item.delete()
        }
    }
}
