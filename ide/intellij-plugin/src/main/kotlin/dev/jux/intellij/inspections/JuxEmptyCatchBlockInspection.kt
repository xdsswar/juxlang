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
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/**
 * "Empty 'catch' block", Java's `CatchMayIgnoreException`: a `catch` whose body
 * does nothing swallows the exception without a trace.
 *
 * As in Java, a comment in the block counts as a deliberate choice, and a
 * parameter named `ignore` or `ignored` says the same thing in code, so
 * neither is reported. The fixes are Java's: rename the parameter to
 * `ignored`, which records the intent, or delete the clause (unwrapping the
 * `try` when nothing else is left to handle).
 */
class JuxEmptyCatchBlockInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.CATCH_CLAUSE) return
                val block = element.node.findChildByType(E.CODE_BLOCK)?.psi ?: return
                if (!isEmpty(block)) return
                val param = element.children.firstOrNull { it.elementType === E.LOCAL_VARIABLE } as? JuxNamedElement
                if (param?.name in IGNORED_NAMES) return
                val keyword = element.node.findChildByType(T.CATCH_KW)?.psi ?: return
                val fixes = ArrayList<LocalQuickFix>()
                if (param?.name != null) fixes.add(RenameToIgnoredFix())
                fixes.add(DeleteCatchFix())
                holder.registerProblem(keyword, "Empty 'catch' block", *fixes.toTypedArray())
            }
        }

    /** No statements and no comments between the braces. */
    private fun isEmpty(block: PsiElement): Boolean =
        JuxCodeFacts.statementsOf(block).isEmpty() && !JuxCodeFacts.hasComment(block)

    /** Renames the `catch` parameter to `ignored` (the body is empty, so nothing else refers to it). */
    private class RenameToIgnoredFix : LocalQuickFix {
        override fun getFamilyName(): String = "Rename 'catch' parameter to 'ignored'"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val clause = descriptor.psiElement?.parent ?: return
            val param = clause.children.firstOrNull { it.elementType === E.LOCAL_VARIABLE } as? JuxNamedElement ?: return
            param.nameIdentifier?.replace(JuxElementFactory.createIdentifier(project, "ignored"))
        }
    }

    /**
     * Deletes the clause. A `try` left with no `catch` and no `finally` is not
     * a statement any more, so its body takes its place.
     */
    private class DeleteCatchFix : LocalQuickFix {
        override fun getFamilyName(): String = "Delete 'catch' clause"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val clause = descriptor.psiElement?.parent ?: return
            val tryStmt = clause.parent ?: return
            val prev = clause.prevSibling
            clause.delete()
            if (prev is com.intellij.psi.PsiWhiteSpace && prev.isValid) prev.delete()
            val remaining = tryStmt.children.any {
                it.elementType === E.CATCH_CLAUSE || it.elementType === E.FINALLY_CLAUSE
            }
            if (!remaining) {
                val body = tryStmt.node.findChildByType(E.CODE_BLOCK)?.psi
                JuxCodeFacts.replaceWithBranch(project, tryStmt, body)
            }
        }
    }

    private companion object {
        /** Parameter names that say "ignored on purpose", as in Java. */
        val IGNORED_NAMES = setOf("ignore", "ignored")
    }
}
