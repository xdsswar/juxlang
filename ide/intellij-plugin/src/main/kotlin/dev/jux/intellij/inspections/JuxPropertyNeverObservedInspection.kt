package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxPropertyDeclaration
import dev.jux.intellij.resolve.JuxPropertyUsages

/**
 * W0971 (§P.7.2): a `{ get; set; }` property that is never observed and never
 * bound may as well be a plain field — the declaration promises observability
 * nobody uses.
 *
 * Scope-honest: the spec suppresses the hint for `public` properties (external
 * code the IDE can't see may observe them). With no cross-project stub index,
 * the same blindness applies to `protected` and default visibility — so only
 * **private** properties are checked, where the in-file scan is *exact*:
 * §P.3.5 ties observer access to getter visibility, so a private property is
 * observable only from this file.
 */
class JuxPropertyNeverObservedInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val usages = JuxPropertyUsages.usagesIn(file)
        val problems = ArrayList<ProblemDescriptor>()

        for (prop in PsiTreeUtil.findChildrenOfType(file, JuxPropertyDeclaration::class.java)) {
            if (!prop.isPrivate()) continue
            // Computed properties exist for derivation, not observation.
            if (prop.isComputed()) continue
            val name = prop.name ?: continue
            if (name in usages.attachSites) continue
            if (name in usages.bindSites) continue
            if (name in usages.bindSources) continue
            val target = prop.nameIdentifier ?: continue

            problems.add(
                manager.createProblemDescriptor(
                    target,
                    "Property '$name' is never observed or bound (W0971) — consider a plain field",
                    null as Array<com.intellij.codeInspection.LocalQuickFix>?,
                    ProblemHighlightType.WEAK_WARNING,
                    isOnTheFly,
                    false,
                ),
            )
        }
        return problems.toTypedArray()
    }
}
