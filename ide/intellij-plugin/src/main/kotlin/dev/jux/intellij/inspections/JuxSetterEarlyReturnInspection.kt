package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxPropertyDeclaration

/**
 * W0973 (§P.7.6): an early `return` in a custom setter body may skip the
 * observer fire — the compiler fires observers after the body completes, so a
 * body that bails out early leaves observers seeing whatever was (or wasn't)
 * set at that point. Every `return` is flagged except one that is the last
 * top-level statement of the block (that one changes nothing). Returns inside
 * lambdas nested in the body belong to the lambda, not the setter — skipped.
 */
class JuxSetterEarlyReturnInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()

        for (prop in PsiTreeUtil.findChildrenOfType(file, JuxPropertyDeclaration::class.java)) {
            val body = prop.setterBody() ?: continue
            val lastStatement = body.children.lastOrNull { it.elementType in STATEMENTS }
            for (ret in collectSetterReturns(body)) {
                // A `return;` that IS the final statement can't skip anything.
                if (ret === lastStatement) continue
                problems.add(
                    manager.createProblemDescriptor(
                        ret.firstChild ?: ret, // anchor on the `return` keyword
                        "Early return may skip the observer fire (W0973)",
                        null as Array<com.intellij.codeInspection.LocalQuickFix>?,
                        ProblemHighlightType.WARNING,
                        isOnTheFly,
                        false,
                    ),
                )
            }
        }
        return problems.toTypedArray()
    }

    /** RETURN_STATEMENTs under [body], not descending into lambdas. */
    private fun collectSetterReturns(body: PsiElement): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        fun walk(e: PsiElement) {
            if (e.elementType === E.LAMBDA_EXPRESSION) return
            if (e.elementType === E.RETURN_STATEMENT) out.add(e)
            var c: PsiElement? = e.firstChild
            while (c != null) {
                walk(c)
                c = c.nextSibling
            }
        }
        walk(body)
        return out
    }

    private companion object {
        /** Statement node types that can terminate a setter block. */
        val STATEMENTS = setOf(
            E.EXPRESSION_STATEMENT, E.LOCAL_VARIABLE, E.IF_STATEMENT,
            E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT, E.FOR_STATEMENT,
            E.FOR_EACH_STATEMENT, E.SWITCH_STATEMENT, E.RETURN_STATEMENT,
            E.BREAK_STATEMENT, E.CONTINUE_STATEMENT, E.THROW_STATEMENT,
            E.TRY_STATEMENT, E.UNSAFE_STATEMENT, E.LABELED_STATEMENT,
            E.EMPTY_STATEMENT, E.CODE_BLOCK,
        )
    }
}
