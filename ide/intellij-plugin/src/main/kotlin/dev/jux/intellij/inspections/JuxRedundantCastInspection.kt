package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * "Redundant cast", as in Java: `(int) count` where `count` is already an
 * `int`. The cast changes nothing, so it is only noise.
 *
 * Reported only when BOTH types are known exactly and are the same simple
 * type ([JuxCodeFacts.sameSimpleType]). A cast involving a pointer, a
 * nullable, an array, a generic or anything the IDE cannot type exactly is
 * never reported, because only the compiler knows whether it converts. An
 * untyped integer literal is an `int` (as the checker reads it), so `(int) 5`
 * is redundant and `(long) 5` is not.
 */
class JuxRedundantCastInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.CAST_EXPRESSION) return
                val typeRef = element.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return
                val operand = JuxTypeEngine.firstExpressionChild(element) ?: return
                if (!JuxCodeFacts.isSimpleTypeText(typeRef.text)) return
                val castTo = JuxTypeEngine.typeOfTypeReference(typeRef)
                val from = JuxCodeFacts.exactType(operand) ?: return
                if (!JuxCodeFacts.sameSimpleType(castTo, from)) return
                // Highlight `(Type)`, as Java does.
                val close = element.node.findChildByType(T.RPAREN)?.psi ?: return
                val range = TextRange(0, close.textRange.endOffset - element.textRange.startOffset)
                holder.registerProblem(
                    element,
                    "Casting '${operand.text}' to '${typeRef.text.trim()}' is redundant",
                    ProblemHighlightType.LIKE_UNUSED_SYMBOL,
                    range,
                    RemoveCastFix(),
                )
            }
        }

    /** Replaces `(T) x` with `x`. */
    private class RemoveCastFix : LocalQuickFix {
        override fun getFamilyName(): String = "Remove redundant cast"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val cast = descriptor.psiElement ?: return
            val operand = JuxTypeEngine.firstExpressionChild(cast) ?: return
            cast.replace(operand.copy())
        }
    }
}
