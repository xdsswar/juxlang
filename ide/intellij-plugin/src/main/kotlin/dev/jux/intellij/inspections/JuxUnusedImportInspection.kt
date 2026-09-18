package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import dev.jux.intellij.editor.JuxImportOptimizer
import dev.jux.intellij.editor.JuxImportSupport
import dev.jux.intellij.editor.JuxImportSupport.Verdict
import dev.jux.intellij.editor.JuxOnTheFlyImportOptimizer
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.settings.JuxCodeInsightWorkspaceSettings

/**
 * Flags imports Optimize Imports would remove, from the same analysis
 * ([JuxImportSupport.analyze]), so the warning and the action always agree:
 *
 *  - an import that binds nothing the file uses, or repeats an earlier one;
 *  - each unused member of a grouped import, marked on the member itself;
 *  - a wildcard import the project index proves nothing is taken from.
 *
 * Each warning offers "Optimize imports" (the whole file, as in Java) and,
 * where a whole statement is unused, "Remove import".
 */
class JuxUnusedImportInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()
        // Offered on every warning while the option is off, as Java does.
        val onTheFly = JuxCodeInsightWorkspaceSettings.getInstance(file.project).optimizeImportsOnTheFly
        fun fixes(vararg first: LocalQuickFix): Array<LocalQuickFix> =
            if (onTheFly) arrayOf(*first) else arrayOf(*first, EnableOptimizeImportsOnTheFlyFix())
        for (d in JuxImportSupport.analyze(file)) {
            when (d.verdict) {
                Verdict.KEEP -> Unit
                Verdict.UNUSED, Verdict.DUPLICATE -> problems.add(
                    manager.createProblemDescriptor(
                        d.import.element,
                        if (d.verdict == Verdict.DUPLICATE) "Duplicate import" else "Unused import",
                        isOnTheFly,
                        fixes(OptimizeImportsFix(), RemoveImportFix()),
                        ProblemHighlightType.LIKE_UNUSED_SYMBOL,
                    ),
                )
                Verdict.PRUNE -> for (item in d.droppedItems) {
                    problems.add(
                        manager.createProblemDescriptor(
                            d.import.element,
                            item.rangeInStatement,
                            "Unused import '${item.boundName}'",
                            ProblemHighlightType.LIKE_UNUSED_SYMBOL,
                            isOnTheFly,
                            *fixes(OptimizeImportsFix()),
                        ),
                    )
                }
            }
        }
        // Something to remove: let the on-the-fly optimizer act once
        // highlighting settles (it checks the option and the moment itself).
        if (isOnTheFly && problems.isNotEmpty()) JuxOnTheFlyImportOptimizer.schedule(file)
        return problems.toTypedArray()
    }

    /** Turns on "Optimize imports on the fly" for the project, then optimizes. */
    class EnableOptimizeImportsOnTheFlyFix : LocalQuickFix {
        override fun getFamilyName(): String = "Enable 'Optimize imports on the fly'"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            JuxCodeInsightWorkspaceSettings.getInstance(project).optimizeImportsOnTheFly = true
            val file = descriptor.psiElement?.containingFile ?: return
            JuxImportOptimizer().processFile(file).run()
        }
    }

    /** Runs Optimize Imports over the file, as the Java inspection's fix does. */
    class OptimizeImportsFix : LocalQuickFix {
        override fun getFamilyName(): String = "Optimize imports"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val file = descriptor.psiElement?.containingFile ?: return
            JuxImportOptimizer().processFile(file).run()
        }
    }

    /** Deletes the import statement and the line break it occupied. */
    private class RemoveImportFix : LocalQuickFix {
        override fun getFamilyName(): String = "Remove import"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val stmt = descriptor.psiElement ?: return
            // Take the trailing newline with the statement so no blank line
            // is left behind.
            val next = stmt.nextSibling
            stmt.delete()
            if (next is PsiWhiteSpace && next.isValid &&
                (next.text.startsWith("\n") || next.text.startsWith("\r"))
            ) {
                next.delete()
            }
        }
    }
}
