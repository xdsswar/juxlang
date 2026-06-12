package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.resolve.JuxPropertyUsages

/**
 * E0973 (§P.4.2 / §P.7.5): a property bound one-way follows its source — a
 * direct assignment fights the binding (and throws at runtime in debug
 * builds). The compiler will own the full-flow version of this check; until
 * then the IDE flags the textually-evident case.
 *
 * Precision rules (these are un-suppressable red errors, so false positives
 * are worse than misses):
 * - the assignment LHS must match a `bind()` receiver by **whole chain text**
 *   (`Label.Name`, not any `Name`) — kills cross-object false positives;
 * - any `unbind()` on the same chain in the file silences the diagnostic
 *   (flow-insensitive: bind-then-unbind-then-set is legal and we can't order);
 * - bidirectional bindings never flag — direct sets are legal there (§P.4.3).
 */
class JuxBoundPropertyAssignmentInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val usages = JuxPropertyUsages.usagesIn(file)
        if (usages.bindTargets.isEmpty()) return null
        val problems = ArrayList<ProblemDescriptor>()

        val assignments = PsiTreeUtil.collectElements(file) {
            it.elementType === E.ASSIGNMENT_EXPRESSION
        }
        for (assign in assignments) {
            val lhs = assign.firstChild ?: continue
            if (lhs.elementType !== E.REFERENCE_EXPRESSION &&
                lhs.elementType !== E.FIELD_ACCESS_EXPRESSION
            ) continue
            val chain = JuxPropertyUsages.chainText(lhs)
            if (chain !in usages.bindTargets) continue
            if (chain in usages.unbindTargets) continue

            problems.add(
                manager.createProblemDescriptor(
                    lhs,
                    "'$chain' is bound — direct assignment is not allowed (E0973)",
                    null as Array<com.intellij.codeInspection.LocalQuickFix>?,
                    ProblemHighlightType.GENERIC_ERROR,
                    isOnTheFly,
                    false,
                ),
            )
        }
        return problems.toTypedArray()
    }
}
