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
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * "Pointless boolean expression", as in Java: a boolean literal where it
 * changes nothing or decides everything.
 *
 * - `b == true` is `b`, `b == false` is `!b`, `b != true` is `!b`;
 * - `b && true` is `b`, `b || false` is `b`;
 * - `b && false` is `false` and `b || true` is `true`, but only when `b` has
 *   no side effects, since the simplification stops evaluating it;
 * - `!true` is `false`, and `! !b` is `b`.
 *
 * The comparisons are only rewritten when the other side is known to be a
 * `bool`: a class's `operator==` may take a bool and mean something else.
 */
class JuxPointlessBooleanExpressionInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                val simplified = when (element.elementType) {
                    E.BINARY_EXPRESSION -> simplifyBinary(element)
                    E.UNARY_EXPRESSION -> simplifyNot(element)
                    else -> null
                } ?: return
                holder.registerProblem(
                    element,
                    "'${element.text}' can be simplified to '$simplified'",
                    SimplifyFix(simplified),
                )
            }
        }

    /** The simpler form of a binary with a boolean literal operand, or null. */
    private fun simplifyBinary(binary: PsiElement): String? {
        val op = JuxCodeFacts.binaryOperator(binary)?.elementType ?: return null
        val (left, right) = JuxCodeFacts.operands(binary).takeIf { it.size == 2 } ?: return null
        val lLit = JuxCodeFacts.boolLiteral(left)
        val rLit = JuxCodeFacts.boolLiteral(right)
        if ((lLit == null) == (rLit == null)) return null // none, or both (a constant condition)
        val literal = lLit ?: rLit!!
        val other = if (lLit != null) right else left
        return when (op) {
            T.EQ_EQ, T.NOT_EQ -> {
                if (!isBool(other)) return null
                val positive = (op === T.EQ_EQ) == literal
                if (positive) other.text else negate(other)
            }
            T.AND_AND -> when {
                literal -> other.text
                JuxCodeFacts.isSideEffectFree(other) -> "false"
                else -> null
            }
            T.OR_OR -> when {
                !literal -> other.text
                JuxCodeFacts.isSideEffectFree(other) -> "true"
                else -> null
            }
            else -> null
        }
    }

    /** `!true` → `false`; `! !b` → `b`. */
    private fun simplifyNot(unary: PsiElement): String? {
        if (unary.firstChild?.elementType !== T.BANG) return null
        val operand = JuxTypeEngine.firstExpressionChild(unary) ?: return null
        JuxCodeFacts.boolLiteral(operand)?.let { return (!it).toString() }
        val inner = JuxCodeFacts.stripParens(operand) ?: return null
        if (inner.elementType === E.UNARY_EXPRESSION && inner.firstChild?.elementType === T.BANG) {
            val x = JuxTypeEngine.firstExpressionChild(inner) ?: return null
            if (isBool(x)) return x.text
        }
        return null
    }

    /** Known to be a `bool` (a `bool`-typed name, a comparison, a `!`, `&&`, `||`). */
    private fun isBool(e: PsiElement): Boolean {
        val t = JuxCodeFacts.exactType(e) ?: JuxTypeEngine.typeOf(e)
        return t is dev.jux.intellij.resolve.JuxType.Primitive && t.name == "bool"
    }

    /** `!x`, parenthesizing anything that is not already atomic. */
    private fun negate(e: PsiElement): String {
        val atomic = e.elementType in ATOMIC
        return if (atomic) "!${e.text}" else "!(${e.text})"
    }

    /** Replaces the expression with its simpler form. */
    private class SimplifyFix(private val replacement: String) : LocalQuickFix {
        override fun getName(): String = "Simplify to '$replacement'"

        override fun getFamilyName(): String = "Simplify boolean expression"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val e = descriptor.psiElement ?: return
            e.replace(JuxElementFactory.createExpression(project, replacement))
        }
    }

    private companion object {
        /** Expressions `!` can prefix without parentheses. */
        val ATOMIC = setOf(
            E.REFERENCE_EXPRESSION, E.LITERAL_EXPRESSION, E.CALL_EXPRESSION, E.FIELD_ACCESS_EXPRESSION,
            E.PARENTHESIZED_EXPRESSION, E.THIS_EXPRESSION, E.INDEX_EXPRESSION,
        )
    }
}
