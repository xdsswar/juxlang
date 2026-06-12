package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.refactoring.RenameRefactoring
import com.intellij.refactoring.RefactoringFactory
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxPropertyDeclaration

/**
 * W0974 (§P.1.1 / §P.7.1): property names are *preferably* PascalCase — the
 * visual signal that a name is a property, not a plain field. A convention,
 * never an error: surfaced as a weak warning with a rename quick-fix, exactly
 * mirroring the compiler's suppressible W0974.
 *
 * The quick-fix goes through the platform rename refactoring (not a manual
 * `setName` + text patching), so the declaration and every usage that
 * *resolves* to the property — reads, writes, `.observers.attach(…)` sites,
 * `.bind(…)` sites — update in one undoable operation. Honest limit: a
 * qualified cross-object usage renames only where the reference resolves
 * (in-file resolution today); unresolvable sites are left alone rather than
 * text-search-patched.
 */
class JuxPropertyNamingInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()

        for (prop in PsiTreeUtil.findChildrenOfType(file, JuxPropertyDeclaration::class.java)) {
            val name = prop.name ?: continue
            // `_`-prefixed names are deliberate backing-field style — skip.
            if (name.startsWith("_")) continue
            if (name.firstOrNull()?.isLowerCase() != true) continue
            val target = prop.nameIdentifier ?: continue

            problems.add(
                manager.createProblemDescriptor(
                    target,
                    "Property '$name' should be PascalCase (W0974)",
                    RenameToPascalCaseFix(),
                    ProblemHighlightType.WEAK_WARNING,
                    isOnTheFly,
                ),
            )
        }
        return problems.toTypedArray()
    }

    /** Renames the property (and all resolving usages) to PascalCase. */
    private class RenameToPascalCaseFix : LocalQuickFix {
        override fun getFamilyName(): String = "Rename property to PascalCase"

        // The rename refactoring manages its own write command (and may show
        // conflict UI); running it inside the quick-fix write action deadlocks.
        override fun startInWriteAction(): Boolean = false

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val prop = descriptor.psiElement?.parent as? JuxPropertyDeclaration ?: return
            val name = prop.name ?: return
            val newName = name.replaceFirstChar { it.uppercaseChar() }
            val refactoring: RenameRefactoring =
                RefactoringFactory.getInstance(project).createRename(prop, newName)
            refactoring.isSearchInComments = false
            refactoring.run()
        }
    }
}
