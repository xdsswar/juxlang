package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFixAndIntentionActionOnPsiElement
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.PsiErrorElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * The compiler's rules for a function declared to return `never` (Core lib
 * K.4.1), checked as you type:
 *
 * - E0485: the body can reach its end. Every path must throw, loop forever,
 *   or call another `never` function. Reported at the name, as the compiler
 *   does. Fix: end the body with a `throw`.
 * - E0486: a `return` in the body. A `return` inside a lambda or an anonymous
 *   class belongs to that, not to the `never` function, and is fine.
 *
 * "Can reach its end" comes from [JuxCodeFacts.canCompleteNormally], which
 * answers "cannot" whenever it is unsure, so a correct body is never flagged.
 */
class JuxNeverFunctionInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.METHOD_DECLARATION) return
                if (JuxHierarchy.returnTypeText(element) != NEVER) return
                val body = element.node.findChildByType(E.CODE_BLOCK)?.psi ?: return
                if (PsiTreeUtil.findChildOfType(body, PsiErrorElement::class.java) != null) return
                val name = (element as? JuxNamedElement)?.name ?: return

                for (ret in ownReturns(body)) {
                    holder.registerProblem(
                        ret,
                        "`$name` returns `never`, so it cannot `return` (E0486)",
                        ProblemHighlightType.GENERIC_ERROR,
                    )
                }
                if (JuxCodeFacts.canCompleteNormally(body)) {
                    val anchor = (element as? JuxNamedElement)?.nameIdentifier ?: return
                    holder.registerProblem(
                        anchor,
                        "`$name` returns `never` but can reach the end of its body (E0485)",
                        ProblemHighlightType.GENERIC_ERROR,
                        AddThrowFix(element),
                    )
                }
            }
        }

    /** The `return` statements that belong to [body]'s own function. */
    private fun ownReturns(body: PsiElement): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        fun visit(x: PsiElement) {
            if (x !== body && (x.elementType === E.LAMBDA_EXPRESSION || x is JuxTypeDeclaration ||
                    x.elementType === E.NEW_EXPRESSION && x.node.findChildByType(E.CLASS_BODY) != null)
            ) return
            if (x.elementType === E.RETURN_STATEMENT) out.add(x)
            var c = x.firstChild
            while (c != null) { visit(c); c = c.nextSibling }
        }
        visit(body)
        return out
    }

    /** Ends the body with a `throw`, the message as the edit point. */
    private class AddThrowFix(method: PsiElement) : LocalQuickFixAndIntentionActionOnPsiElement(method) {
        override fun getText(): String = "Add 'throw' at the end"

        override fun getFamilyName(): String = "Add 'throw' at the end"

        override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
            val body = startElement.node.findChildByType(E.CODE_BLOCK)?.psi ?: return
            JuxCodeFacts.addStatementAtEnd(project, body, "throw new IllegalStateException(\"unreachable\");")
        }
    }

    private companion object {
        const val NEVER = "never"
    }
}
