package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * "Variable is assigned to itself", Java's `SillyAssignment`: `x = x;` does
 * nothing, and is usually a constructor that meant `this.x = x`.
 *
 * Both sides must name the same local, parameter or field, spelled the same
 * way (`x = x`, `this.x = this.x`). A property is left out: its setter runs
 * on assignment and may do something (validate, notify observers). The fix
 * deletes the statement.
 */
class JuxSelfAssignmentInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.ASSIGNMENT_EXPRESSION) return
                if (JuxCodeFacts.binaryOperator(element)?.elementType !== T.EQ) return
                val (left, right) = JuxTypeEngine.expressionChildren(element).takeIf { it.size == 2 } ?: return
                if (!JuxCodeFacts.isPlainRead(left) || !JuxCodeFacts.isPlainRead(right)) return
                if (JuxCodeFacts.normalizedText(left) != JuxCodeFacts.normalizedText(right)) return
                if (!namesPlainStorage(left)) return
                val fixes = if (element.parent?.elementType === E.EXPRESSION_STATEMENT) {
                    arrayOf<LocalQuickFix>(RemoveStatementFix())
                } else LocalQuickFix.EMPTY_ARRAY
                holder.registerProblem(element, "Variable '${left.text}' is assigned to itself", *fixes)
            }
        }

    /** A local, parameter or plain field: somewhere an assignment only stores. */
    private fun namesPlainStorage(target: PsiElement): Boolean {
        val s = JuxCodeFacts.stripParens(target) ?: return false
        val decl = when (s.elementType) {
            E.REFERENCE_EXPRESSION -> JuxTypeEngine.resolveReferenceExpression(s)
            E.FIELD_ACCESS_EXPRESSION -> JuxTypeEngine.resolveMemberAccess(s)?.element
            else -> null
        } ?: return false
        return decl.elementType in STORAGE
    }

    /** Deletes the do-nothing statement. */
    private class RemoveStatementFix : LocalQuickFix {
        override fun getFamilyName(): String = "Remove self-assignment"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val stmt = descriptor.psiElement?.parent ?: return
            JuxCodeFacts.replaceWithBranch(project, stmt, null)
        }
    }

    private companion object {
        val STORAGE = setOf(E.LOCAL_VARIABLE, E.PARAMETER, E.FIELD_DECLARATION)
    }
}
