package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.intentions.JuxCodeShapes as S
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * "'if' statement can be simplified", Java's redundant-if check: an `if`
 * whose branches only return (or assign) `true` and `false` is its condition.
 *
 * - `if (c) return true; else return false;` becomes `return c;`
 * - `if (c) { return false; } return true;` becomes `return !c;`
 * - `if (c) x = true; else x = false;` becomes `x = c;`
 *
 * Braces around a single statement do not matter; a branch holding anything
 * else (a comment included) is left alone.
 */
class JuxSimplifiableIfInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.IF_STATEMENT) return
                val shape = shapeOf(element) ?: return
                val kw = S.token(element, T.IF_KW) ?: return
                holder.registerProblem(kw, "'if' statement can be simplified",
                    ProblemHighlightType.GENERIC_ERROR_OR_WARNING, SimplifyFix(shape.replacement))
            }
        }

    /** What the `if` (and, for the no-`else` form, the statement after it) becomes. */
    private data class Shape(val replacement: String)

    private fun shapeOf(ifStmt: PsiElement): Shape? {
        val cond = S.ifCondition(ifStmt) ?: return null
        val then = S.ifThen(ifStmt)?.let { S.singleStatement(it) } ?: return null
        val elseBranch = S.ifElse(ifStmt)
        val otherwise: PsiElement
        if (elseBranch != null) {
            // `else if` is a longer chain, not this shape.
            if (elseBranch.elementType === E.IF_STATEMENT) return null
            otherwise = S.singleStatement(elseBranch) ?: return null
        } else {
            // `if (c) return true; return false;`: the next statement is the else.
            otherwise = nextStatement(ifStmt)?.takeIf { it.elementType === E.RETURN_STATEMENT } ?: return null
            if (then.elementType !== E.RETURN_STATEMENT) return null
        }
        // Both branches return a boolean literal.
        val a = returnedLiteral(then)
        val b = returnedLiteral(otherwise)
        if (a != null && b != null && a != b) {
            return Shape("return ${if (a == "true") S.unparenthesized(cond).text else S.negate(cond)};")
        }
        // Both branches assign a boolean literal to the same target.
        val (ta, va) = assignedLiteral(then) ?: return null
        val (tb, vb) = assignedLiteral(otherwise) ?: return null
        if (elseBranch == null || ta != tb || va == vb) return null
        return Shape("$ta = ${if (va == "true") S.unparenthesized(cond).text else S.negate(cond)};")
    }

    /** `true` / `false` of `return true;`, or null. */
    private fun returnedLiteral(stmt: PsiElement): String? {
        if (stmt.elementType !== E.RETURN_STATEMENT) return null
        val value = S.compositeChildren(stmt).singleOrNull() ?: return null
        return value.text.takeIf { value.elementType === E.LITERAL_EXPRESSION && (it == "true" || it == "false") }
    }

    /** `(x, "true")` of `x = true;`, or null. */
    private fun assignedLiteral(stmt: PsiElement): Pair<String, String>? {
        if (stmt.elementType !== E.EXPRESSION_STATEMENT) return null
        val assign = S.compositeChildren(stmt).singleOrNull()?.takeIf { it.elementType === E.ASSIGNMENT_EXPRESSION } ?: return null
        if (S.token(assign, T.EQ) == null) return null
        val parts = S.compositeChildren(assign)
        if (parts.size != 2) return null
        val value = parts[1].takeIf { it.elementType === E.LITERAL_EXPRESSION && (it.text == "true" || it.text == "false") }
            ?: return null
        return parts[0].text to value.text
    }

    /** The statement right after [stmt] in its block, skipping whitespace; null when a comment sits between. */
    private fun nextStatement(stmt: PsiElement): PsiElement? {
        var c = stmt.nextSibling
        while (c is PsiWhiteSpace) c = c.nextSibling
        if (c is PsiComment) return null
        return c
    }

    private class SimplifyFix(private val replacement: String) : LocalQuickFix {
        override fun getFamilyName(): String = "Simplify 'if' statement"
        override fun getName(): String = "Replace 'if' with '$replacement'"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val ifStmt = descriptor.psiElement?.parent ?: return
            val end = if (S.ifElse(ifStmt) != null) ifStmt.textRange.endOffset
            else nextStatement(ifStmt)?.textRange?.endOffset ?: return
            S.replace(ifStmt, TextRange(ifStmt.textRange.startOffset, end), replacement)
        }

        private fun nextStatement(stmt: PsiElement): PsiElement? {
            var c = stmt.nextSibling
            while (c is PsiWhiteSpace) c = c.nextSibling
            return c
        }
    }
}
