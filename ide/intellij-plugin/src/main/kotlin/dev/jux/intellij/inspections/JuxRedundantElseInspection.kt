package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * "Redundant 'else'", Java's `ConfusingElse`: when the `if` branch always
 * jumps away (`return`, `throw`, `break`, `continue`), the `else` only adds
 * nesting; its statements can follow the `if` directly.
 *
 * Registered at INFORMATION level like Java's (no highlight, offered on
 * Alt+Enter over the `else`): a guard written with an `else` is a style
 * choice, not a mistake. Reported only when the `if` branch CERTAINLY
 * jumps ([JuxCodeFacts.definitelyJumps]) and the chain sits directly in a
 * block, so the fix can move the statements after it.
 */
class JuxRedundantElseInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.IF_STATEMENT) return
                val then = JuxCodeFacts.thenBranch(element) ?: return
                JuxCodeFacts.elseBranch(element) ?: return
                if (!JuxCodeFacts.definitelyJumps(then)) return
                if (chainAnchor(element) == null) return
                val elseKw = JuxCodeFacts.elseKeyword(element) ?: return
                holder.registerProblem(elseKw, "Redundant 'else'", UnwrapElseFix())
            }
        }

    /** Removes `else`, moving its statements after the whole `if` chain. */
    private class UnwrapElseFix : LocalQuickFix {
        override fun getFamilyName(): String = "Remove redundant 'else'"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val ifStmt = descriptor.psiElement?.parent ?: return
            val elseBranch = JuxCodeFacts.elseBranch(ifStmt) ?: return
            val elseKw = JuxCodeFacts.elseKeyword(ifStmt) ?: return
            val anchor = chainAnchor(ifStmt) ?: return
            // What the else runs (comments included), as written.
            val moved: List<String> =
                if (elseBranch.elementType === E.CODE_BLOCK) JuxCodeFacts.blockContents(elseBranch).map { it.text }
                else listOf(elseBranch.text)
            // Drop the `else` and its branch (and the space before `else`),
            // then write the statements after the chain, each on its own line.
            val before = elseKw.prevSibling
            ifStmt.deleteChildRange(elseKw, elseBranch)
            if (before is PsiWhiteSpace && before.isValid) before.delete()
            JuxCodeFacts.insertLinesAfter(project, anchor, moved)
        }
    }

    private companion object {
        /**
         * The outermost `if` of the `else if` chain [ifStmt] belongs to, when
         * that `if` sits directly in a code block; null otherwise.
         *
         * Every `if` above [ifStmt] in the chain must jump away too: in
         * `if (a) y(); else if (b) return; else x();` moving `x()` after the
         * chain would run it after `y()` as well.
         */
        fun chainAnchor(ifStmt: PsiElement): PsiElement? {
            var anchor = ifStmt
            while (anchor.parent?.elementType === E.IF_STATEMENT) {
                val outer = anchor.parent
                val outerThen = JuxCodeFacts.thenBranch(outer) ?: return null
                if (!JuxCodeFacts.definitelyJumps(outerThen)) return null
                anchor = outer
            }
            return anchor.takeIf { it.parent?.elementType === E.CODE_BLOCK }
        }
    }
}
