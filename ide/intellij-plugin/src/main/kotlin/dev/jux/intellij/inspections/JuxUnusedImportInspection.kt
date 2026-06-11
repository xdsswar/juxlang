package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import dev.jux.intellij.editor.JuxImportSupport
import dev.jux.intellij.psi.JuxFile

/**
 * Flags `import` statements that bind no name the file references, and exact
 * duplicates — the same analysis Optimize Imports applies
 * ([JuxImportSupport]), surfaced as live warnings with a removal quick-fix.
 * Wildcard / side-effect imports are never flagged (their usage can't be
 * proven from this file alone).
 */
class JuxUnusedImportInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val imports = JuxImportSupport.collectImports(file)
        if (imports.isEmpty()) return null
        val used = JuxImportSupport.collectUsedNames(file, imports)

        val problems = ArrayList<ProblemDescriptor>()
        val seen = HashSet<String>()
        for (imp in imports) {
            val message = when {
                !seen.add(imp.dedupKey) -> "Duplicate import"
                !imp.alwaysKeep && imp.boundNames.none { it in used } -> "Unused import"
                else -> continue
            }
            problems.add(
                manager.createProblemDescriptor(
                    imp.element,
                    message,
                    RemoveImportFix(),
                    ProblemHighlightType.LIKE_UNUSED_SYMBOL,
                    isOnTheFly,
                ),
            )
        }
        return problems.toTypedArray()
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
            if (next is PsiWhiteSpace && next.isValid && next.text.startsWith("\n")) {
                next.delete()
            }
        }
    }
}
