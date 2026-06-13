package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.codeInsight.JuxOverrideMembers
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * E0429 mirrored IDE-side: a **non-abstract class** must implement every
 * abstract method it inherits (interface methods without a `default` body,
 * `abstract` methods of an abstract superclass). The Java-plugin red squiggle
 * on the class name, with an "Implement methods" quick-fix that inserts the
 * stubs through the shared [JuxOverrideMembers] engine.
 *
 * The engine resolves supertypes project-wide ([dev.jux.intellij.resolve
 * .JuxTypeIndex]) and stays silent on unresolved names (std / library types),
 * so valid code never gets a false error. A method abstract in an interface
 * but implemented by a nearer concrete ancestor counts as satisfied
 * (nearest-declaration-wins in the engine's walk).
 */
class JuxAbstractNotImplementedInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()

        for (type in PsiTreeUtil.findChildrenOfType(file, JuxTypeDeclaration::class.java)) {
            // Only concrete classes owe implementations; interfaces and
            // abstract classes pass the obligation down. Records/enums can
            // carry implements clauses too — the engine walk covers them the
            // same way.
            if (JuxHierarchy.isInterface(type)) continue
            if (JuxHierarchy.isAbstractType(type)) continue
            val name = type.name ?: continue
            val target = type.nameIdentifier ?: continue

            val missing = JuxOverrideMembers.candidates(type)
                .filter { it.kind == JuxOverrideMembers.Kind.IMPLEMENT }
            if (missing.isEmpty()) continue

            // Same sentence shape as the compiler's E0429.
            val list = missing
                .map { "'${it.ownerName}.${it.method.name}'" }
                .distinct()
                .sorted()
                .joinToString(", ")
            problems.add(
                manager.createProblemDescriptor(
                    target,
                    "Class '$name' doesn't implement abstract method(s): $list (E0429)",
                    ImplementMethodsFix(),
                    ProblemHighlightType.ERROR,
                    isOnTheFly,
                ),
            )
        }
        return problems.toTypedArray()
    }

    /**
     * Inserts stubs for ALL missing abstract methods (no chooser inside a
     * quick-fix); candidates are recomputed in [applyFix] so the descriptor
     * never applies stale PSI.
     */
    private class ImplementMethodsFix : LocalQuickFix {
        override fun getFamilyName(): String = "Implement methods"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val type = descriptor.psiElement?.parent as? JuxTypeDeclaration ?: return
            val missing = JuxOverrideMembers.candidates(type)
                .filter { it.kind == JuxOverrideMembers.Kind.IMPLEMENT }
            JuxOverrideMembers.insertStubs(project, type, missing)
        }
    }
}
