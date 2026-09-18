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
 * "Constant condition", the part of Java's constant-conditions analysis the
 * IDE can decide without a data-flow engine:
 *
 * - an `if`, `?:` or `while` whose condition is the literal `true` / `false`
 *   (`while (true)` and `do … while (false)` are idioms and are left alone);
 * - a value compared with itself (`x == x`, `n < n`), Java's
 *   `ExpressionComparedToItself`: only for a plain read of a primitive whose
 *   `==` is reflexive, never a float (`NaN != NaN` is how a NaN test is
 *   written), never a class (its `operator==` may say anything);
 * - two literals compared (`1 < 2`, `'a' == 'b'`).
 *
 * The fix replaces the condition's user with what always happens: the branch
 * that runs, nothing for a loop that never does, or the literal result.
 */
class JuxConstantConditionInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                when (element.elementType) {
                    E.IF_STATEMENT -> {
                        val cond = JuxCodeFacts.conditionOf(element) ?: return
                        val value = JuxCodeFacts.boolLiteral(cond) ?: return
                        holder.registerProblem(cond, "Condition is always $value", SimplifyIfFix(value))
                    }
                    E.WHILE_STATEMENT -> {
                        val cond = JuxCodeFacts.conditionOf(element) ?: return
                        if (JuxCodeFacts.boolLiteral(cond) != false) return
                        holder.registerProblem(cond, "Condition is always false", RemoveLoopFix())
                    }
                    E.CONDITIONAL_EXPRESSION -> {
                        val cond = JuxTypeEngine.firstExpressionChild(element) ?: return
                        val value = JuxCodeFacts.boolLiteral(cond) ?: return
                        holder.registerProblem(cond, "Condition is always $value", SimplifyConditionalFix(value))
                    }
                    E.BINARY_EXPRESSION -> checkComparison(element, holder)
                }
            }
        }

    private fun checkComparison(binary: PsiElement, holder: ProblemsHolder) {
        val op = JuxCodeFacts.binaryOperator(binary)?.elementType ?: return
        if (op !in COMPARISONS) return
        val (left, right) = JuxCodeFacts.operands(binary).takeIf { it.size == 2 } ?: return
        val value = selfComparison(op, left, right) ?: literalComparison(op, left, right) ?: return
        holder.registerProblem(binary, "Condition '${binary.text}' is always $value", ReplaceWithLiteralFix(value))
    }

    /** `x == x` and friends: the answer every reflexive comparison gives. */
    private fun selfComparison(op: com.intellij.psi.tree.IElementType, left: PsiElement, right: PsiElement): Boolean? {
        if (!JuxCodeFacts.isPlainRead(left) || !JuxCodeFacts.isPlainRead(right)) return null
        if (JuxCodeFacts.normalizedText(left) != JuxCodeFacts.normalizedText(right)) return null
        if (!JuxCodeFacts.isReflexivePrimitive(JuxCodeFacts.exactType(left))) return null
        return op === T.EQ_EQ || op === T.LE || op === T.GE
    }

    /** `1 < 2`: two suffix-free integer literals, two chars, or two bools compared. */
    private fun literalComparison(op: com.intellij.psi.tree.IElementType, left: PsiElement, right: PsiElement): Boolean? {
        val l = JuxCodeFacts.literalToken(left) ?: return null
        val r = JuxCodeFacts.literalToken(right) ?: return null
        if (l.elementType !== r.elementType) return null
        val cmp: Int = when (l.elementType) {
            T.INT_LITERAL -> {
                val a = l.text.replace("_", "").toLongOrNull() ?: return null
                val b = r.text.replace("_", "").toLongOrNull() ?: return null
                a.compareTo(b)
            }
            T.CHAR_LITERAL -> {
                // Only a plain one-character literal; escapes are left to the compiler.
                if (l.text.length != 3 || r.text.length != 3) return null
                l.text[1].compareTo(r.text[1])
            }
            T.BOOL_LITERAL -> {
                if (op !== T.EQ_EQ && op !== T.NOT_EQ) return null
                if (l.text == r.text) 0 else 1
            }
            else -> return null
        }
        return when (op) {
            T.EQ_EQ -> cmp == 0
            T.NOT_EQ -> cmp != 0
            T.LT -> cmp < 0
            T.LE -> cmp <= 0
            T.GT -> cmp > 0
            T.GE -> cmp >= 0
            else -> null
        }
    }

    /** `if (true) A else B` becomes A; `if (false) A else B` becomes B (or nothing). */
    private class SimplifyIfFix(private val value: Boolean) : LocalQuickFix {
        override fun getFamilyName(): String = "Simplify 'if' with constant condition"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val ifStmt = descriptor.psiElement?.parent ?: return
            val keep = if (value) JuxCodeFacts.thenBranch(ifStmt) else JuxCodeFacts.elseBranch(ifStmt)
            JuxCodeFacts.replaceWithBranch(project, ifStmt, keep)
        }
    }

    /** A `while (false)` loop never runs: delete it. */
    private class RemoveLoopFix : LocalQuickFix {
        override fun getFamilyName(): String = "Remove loop that never runs"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val loop = descriptor.psiElement?.parent ?: return
            JuxCodeFacts.replaceWithBranch(project, loop, null)
        }
    }

    /** `true ? a : b` becomes `a`. */
    private class SimplifyConditionalFix(private val value: Boolean) : LocalQuickFix {
        override fun getFamilyName(): String = "Simplify conditional expression"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val conditional = descriptor.psiElement?.parent ?: return
            val parts = JuxTypeEngine.expressionChildren(conditional)
            val keep = parts.getOrNull(if (value) 1 else 2) ?: return
            conditional.replace(keep.copy())
        }
    }

    /** Replaces an always-true/false comparison with its value. */
    private class ReplaceWithLiteralFix(private val value: Boolean) : LocalQuickFix {
        override fun getName(): String = "Replace with '$value'"

        override fun getFamilyName(): String = "Replace with constant value"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val e = descriptor.psiElement ?: return
            e.replace(JuxElementFactory.createExpression(project, value.toString()))
        }
    }

    private companion object {
        val COMPARISONS = setOf(T.EQ_EQ, T.NOT_EQ, T.LT, T.LE, T.GT, T.GE)
    }
}
