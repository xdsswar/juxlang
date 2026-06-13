package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile

/**
 * Flags statements that can never run: any statement that directly follows an
 * unconditional `return` / `throw` / `break` / `continue` in the same block.
 *
 * Deliberately conservative — it only considers statements that are DIRECT
 * children of a code block, so a `return` nested in an `if`/loop never makes
 * the code after the enclosing block look dead. That keeps it free of the
 * false positives a real control-flow analysis would need the compiler for;
 * the quick-fix removes the dead tail.
 */
class JuxUnreachableCodeInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()

        for (block in PsiTreeUtil.findChildrenOfType(file, PsiElement::class.java)) {
            if (block.elementType !== E.CODE_BLOCK) continue
            var terminalSeen = false
            for (child in block.children) {
                val t = child.elementType
                if (t !in STATEMENT_TYPES) continue // skip braces / whitespace / comments
                if (terminalSeen) {
                    problems.add(
                        manager.createProblemDescriptor(
                            child,
                            "Unreachable code",
                            RemoveUnreachableFix(),
                            ProblemHighlightType.LIKE_UNUSED_SYMBOL,
                            isOnTheFly,
                        ),
                    )
                    break // one report per block; the fix clears the whole tail
                }
                if (t in TERMINAL_TYPES) terminalSeen = true
            }
        }
        return problems.toTypedArray()
    }

    /** Deletes the flagged statement and every statement after it, up to `}`. */
    private class RemoveUnreachableFix : LocalQuickFix {
        override fun getFamilyName(): String = "Remove unreachable code"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            // Collect the dead tail first, then delete — deleting a statement
            // can re-balance/invalidate the adjacent whitespace we'd otherwise
            // be holding as the loop cursor.
            val toDelete = ArrayList<PsiElement>()
            var cur: PsiElement? = descriptor.psiElement ?: return
            while (cur != null && cur.elementType !== JuxTokenTypes.RBRACE) {
                toDelete.add(cur)
                cur = cur.nextSibling
            }
            for (el in toDelete) if (el.isValid) el.delete()
        }
    }

    private companion object {
        val TERMINAL_TYPES = setOf(
            E.RETURN_STATEMENT, E.THROW_STATEMENT, E.BREAK_STATEMENT, E.CONTINUE_STATEMENT,
        )

        /** Element types that represent executable statements inside a block. */
        val STATEMENT_TYPES = setOf(
            E.EXPRESSION_STATEMENT, E.IF_STATEMENT, E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT,
            E.FOR_STATEMENT, E.FOR_EACH_STATEMENT, E.SWITCH_STATEMENT, E.RETURN_STATEMENT,
            E.BREAK_STATEMENT, E.CONTINUE_STATEMENT, E.THROW_STATEMENT, E.TRY_STATEMENT,
            E.UNSAFE_STATEMENT, E.LABELED_STATEMENT, E.EMPTY_STATEMENT, E.CODE_BLOCK,
            E.LOCAL_VARIABLE,
        )
    }
}
