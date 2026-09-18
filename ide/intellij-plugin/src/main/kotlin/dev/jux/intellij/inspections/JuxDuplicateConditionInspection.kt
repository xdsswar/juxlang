package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * "Duplicate condition in 'if' statement", as in Java: in an `if` / `else if`
 * chain, a condition already tested by an earlier branch can never be the one
 * that decides, so its branch is dead.
 *
 * Only side-effect-free conditions are compared (no calls, assignments,
 * `++`), after removing whitespace and comments: two calls to the same
 * function may well answer differently. The fix removes the dead branch.
 */
class JuxDuplicateConditionInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.IF_STATEMENT) return
                // Start at the head of a chain; nested `else if`s are walked from there.
                if (element.parent?.elementType === E.IF_STATEMENT &&
                    JuxCodeFacts.elseBranch(element.parent) === element
                ) return
                val seen = HashSet<String>()
                var cur: PsiElement? = element
                while (cur != null && cur.elementType === E.IF_STATEMENT) {
                    val cond = JuxCodeFacts.conditionOf(cur)
                    if (cond != null && JuxCodeFacts.isSideEffectFree(cond)) {
                        val key = JuxCodeFacts.normalizedText(JuxCodeFacts.stripParens(cond) ?: cond)
                        if (!seen.add(key)) {
                            holder.registerProblem(cond, "Duplicate condition '${cond.text}'", RemoveBranchFix())
                        }
                    }
                    cur = JuxCodeFacts.elseBranch(cur)
                }
            }
        }

    /**
     * Removes the dead `else if` branch: the nested `if` is replaced by its own
     * `else` part, or the `else` goes away when there is none.
     */
    private class RemoveBranchFix : LocalQuickFix {
        override fun getFamilyName(): String = "Remove branch with duplicate condition"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val deadIf = descriptor.psiElement?.parent ?: return
            val outer = deadIf.parent ?: return
            if (outer.elementType !== E.IF_STATEMENT) return
            val rest = JuxCodeFacts.elseBranch(deadIf)
            if (rest != null) {
                JuxCodeFacts.reformat(project, deadIf.replace(rest.copy()))
            } else {
                val elseKw = JuxCodeFacts.elseKeyword(outer) ?: return
                val before = elseKw.prevSibling
                outer.deleteChildRange(elseKw, deadIf)
                if (before is com.intellij.psi.PsiWhiteSpace && before.isValid) before.delete()
            }
        }
    }
}
