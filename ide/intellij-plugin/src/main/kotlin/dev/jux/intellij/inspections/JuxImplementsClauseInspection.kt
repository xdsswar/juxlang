package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * E0424 mirrored IDE-side: only **interfaces** appear in an `implements`
 * clause. Implementing a class/record/enum is an error (quick-fix: move a
 * class to the free `extends` slot); an interface declaration never has an
 * `implements` clause at all (interfaces extend other interfaces).
 *
 * Resolution via [JuxTypeIndex]; unresolved names stay silent (std / library).
 */
class JuxImplementsClauseInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()

        for (type in PsiTreeUtil.findChildrenOfType(file, JuxTypeDeclaration::class.java)) {
            val refs = JuxHierarchy.supertypeReferences(type).filter { !it.second }.map { it.first }
            if (refs.isEmpty()) continue
            val typeName = type.name ?: continue

            // `implements` on an interface is structurally wrong (§6.2).
            if (JuxHierarchy.isInterface(type)) {
                for (ref in refs) {
                    problems.add(
                        manager.createProblemDescriptor(
                            ref,
                            "Interfaces extend other interfaces — 'implements' is not allowed " +
                                "on an interface declaration",
                            null as com.intellij.codeInspection.LocalQuickFix?,
                            ProblemHighlightType.ERROR, isOnTheFly,
                        ),
                    )
                }
                continue
            }

            val hasExtends = JuxHierarchy.supertypeReferences(type).any { it.second }
            for (ref in refs) {
                val target = JuxTypeIndex.findType(type.project, JuxHierarchy.bareTypeName(ref)) ?: continue
                if (JuxHierarchy.isInterface(target)) continue
                val message =
                    "Class '$typeName' cannot implement '${target.name}' because it is " +
                        "${JuxHierarchy.kindWord(target)} (only interfaces appear in 'implements') (E0424)"
                // Offer the move only when the target is a class AND the
                // single extends slot is still free.
                val fix = if (JuxHierarchy.isClass(target) && !hasExtends) MoveToExtendsFix() else null
                problems.add(
                    manager.createProblemDescriptor(ref, message, fix, ProblemHighlightType.ERROR, isOnTheFly),
                )
            }
        }
        return problems.toTypedArray()
    }
}
