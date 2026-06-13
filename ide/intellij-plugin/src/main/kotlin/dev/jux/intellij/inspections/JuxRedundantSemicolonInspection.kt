package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile

/**
 * A bare `;` that forms an [E.EMPTY_STATEMENT] is a no-op — flagged as a weak
 * warning with a "Remove redundant semicolon" fix. Only empty statements are
 * touched; the `;` that terminates a real statement, or the separators in a
 * `for (;;)` header, are never empty statements, so they're left alone.
 */
class JuxRedundantSemicolonInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()
        for (el in PsiTreeUtil.findChildrenOfType(file, PsiElement::class.java)) {
            if (el.elementType !== E.EMPTY_STATEMENT) continue
            if (el.textRange.isEmpty) continue // never anchor a descriptor on a 0-width node
            problems.add(
                manager.createProblemDescriptor(
                    el,
                    "Redundant semicolon",
                    RemoveSemicolonFix(),
                    ProblemHighlightType.LIKE_UNUSED_SYMBOL,
                    isOnTheFly,
                ),
            )
        }
        return problems.toTypedArray()
    }

    private class RemoveSemicolonFix : LocalQuickFix {
        override fun getFamilyName(): String = "Remove redundant semicolon"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val stmt = descriptor.psiElement ?: return
            val next = stmt.nextSibling
            stmt.delete()
            // Drop a leftover blank line so removal leaves no gap.
            if (next is PsiWhiteSpace && next.isValid && next.text.startsWith("\n")) {
                next.delete()
            }
        }
    }
}
