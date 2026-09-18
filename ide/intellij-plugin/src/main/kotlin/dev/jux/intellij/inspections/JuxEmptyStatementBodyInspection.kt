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
 * "Statement has empty body", Java's `EmptyStatementBody`: `if (ready);`
 * almost always has a stray semicolon, and `if (ready) {}` does nothing.
 *
 * Covers `if`, `while`, `for` and for-each bodies. A body holding a comment
 * says the emptiness is on purpose and is not reported; a `while` with a
 * block body `{}` is left alone too, since `while (poll()) {}` is how a
 * condition with side effects is spun. An `if` with an `else` has work in
 * the `else`, so only its then-part is checked when it is a lone `;`.
 *
 * For an empty `if` with no `else` and a side-effect-free condition, the
 * fix removes the whole statement.
 */
class JuxEmptyStatementBodyInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                val (body, keyword) = when (element.elementType) {
                    E.IF_STATEMENT -> JuxCodeFacts.thenBranch(element) to "if"
                    E.WHILE_STATEMENT -> JuxCodeFacts.loopBody(element) to "while"
                    E.FOR_STATEMENT, E.FOR_EACH_STATEMENT -> JuxCodeFacts.loopBody(element) to "for"
                    else -> return
                }
                body ?: return
                val emptyStatement = body.elementType === E.EMPTY_STATEMENT
                val emptyBlock = body.elementType === E.CODE_BLOCK &&
                    JuxCodeFacts.statementsOf(body).isEmpty() && !JuxCodeFacts.hasComment(body)
                if (!emptyStatement && !emptyBlock) return
                if (keyword == "while" && emptyBlock) return
                // `if (c) {} else {…}` is an awkward negation, not an empty statement.
                if (keyword == "if" && emptyBlock && JuxCodeFacts.elseBranch(element) != null) return
                val anchor = element.firstChild ?: return
                val fixes = if (keyword == "if" && JuxCodeFacts.elseBranch(element) == null &&
                    JuxCodeFacts.isSideEffectFree(JuxCodeFacts.conditionOf(element))
                ) arrayOf<LocalQuickFix>(RemoveStatementFix()) else LocalQuickFix.EMPTY_ARRAY
                holder.registerProblem(anchor, "'$keyword' statement has empty body", *fixes)
            }
        }

    /** Deletes an `if` that does nothing. */
    private class RemoveStatementFix : LocalQuickFix {
        override fun getFamilyName(): String = "Remove 'if' statement"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val stmt = descriptor.psiElement?.parent ?: return
            JuxCodeFacts.replaceWithBranch(project, stmt, null)
        }
    }
}
