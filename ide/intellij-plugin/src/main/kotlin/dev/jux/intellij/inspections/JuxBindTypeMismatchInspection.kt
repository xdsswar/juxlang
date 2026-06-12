package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.psi.PsiFile
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.resolve.JuxPropertyUsages

/**
 * E0974 (§P.4.3 / §P.7.4): `bind` / `bindBidirectional` require the two
 * properties to have the same type — `String` cannot follow `double`. Flagged
 * inline before compilation, on the argument.
 *
 * Precision-first: the diagnostic fires only when BOTH operands resolve to
 * property declarations (in-file scope walk for bare/`this.` names; qualifier
 * typing through [dev.jux.intellij.resolve.JuxTypeIndex] for `q.Name`) and
 * their declared types differ textually. Anything unresolvable stays silent —
 * the compiler's own E0974 will own the rest once implemented.
 */
class JuxBindTypeMismatchInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val usages = JuxPropertyUsages.usagesIn(file)
        if (usages.bindSites.isEmpty()) return null
        val problems = ArrayList<ProblemDescriptor>()

        for (calls in usages.bindSites.values) {
            for (call in calls) {
                val callee = call.firstChild ?: continue
                val recv = callee.firstChild ?: continue
                val arg = JuxPropertyUsages.bindArgument(call) ?: continue

                val target = JuxPropertyUsages.resolveProperty(recv) ?: continue
                val source = JuxPropertyUsages.resolveProperty(arg) ?: continue
                val targetType = target.typeText() ?: continue
                val sourceType = source.typeText() ?: continue
                if (targetType == sourceType) continue

                problems.add(
                    manager.createProblemDescriptor(
                        arg,
                        "'$sourceType' cannot bind to '$targetType' (E0974)",
                        null as Array<com.intellij.codeInspection.LocalQuickFix>?,
                        ProblemHighlightType.GENERIC_ERROR,
                        isOnTheFly,
                        false,
                    ),
                )
            }
        }
        return problems.toTypedArray()
    }
}
