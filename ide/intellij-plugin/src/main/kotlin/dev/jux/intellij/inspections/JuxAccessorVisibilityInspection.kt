package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IElementType
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxPropertyDeclaration

/**
 * E0972 (§P.1.3 / §M.7.7): a property's setter may not be **more visible**
 * than its getter — `public String Bad { private get; set; }` reads narrower
 * than it writes, which inverts the encapsulation contract. The compiler emits
 * this too; the IDE-side check is exact (pure structure, in-file) and paints
 * the squiggle without a build.
 *
 * Effective accessor visibility = the accessor's own modifier when present,
 * otherwise the property's declared visibility (absent = Jux default, ranked
 * between public and protected).
 */
class JuxAccessorVisibilityInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()

        for (prop in PsiTreeUtil.findChildrenOfType(file, JuxPropertyDeclaration::class.java)) {
            val getter = prop.getterAccessor() ?: continue
            val setter = prop.setterAccessor() ?: continue
            val propRank = rank(prop.propertyVisibility())
            val getterRank = prop.accessorVisibility(getter)?.let(::rank) ?: propRank
            val setterRank = prop.accessorVisibility(setter)?.let(::rank) ?: propRank
            if (setterRank <= getterRank) continue

            val target = prop.accessorVisibility(setter)
                ?.let { setter } // explicit modifier on the setter — flag the accessor
                ?: prop.nameIdentifier ?: continue
            problems.add(
                manager.createProblemDescriptor(
                    target,
                    "Setter visibility exceeds getter visibility (E0972)",
                    null as Array<com.intellij.codeInspection.LocalQuickFix>?,
                    ProblemHighlightType.GENERIC_ERROR,
                    isOnTheFly,
                    false,
                ),
            )
        }
        return problems.toTypedArray()
    }

    /** Visibility rank, wider = higher. Absent modifier = Jux default (3). */
    private fun rank(vis: IElementType?): Int = when (vis) {
        JuxTokenTypes.PUBLIC_KW -> 4
        null -> 3
        JuxTokenTypes.PROTECTED_KW -> 2
        JuxTokenTypes.PRIVATE_KW -> 1
        else -> 3
    }
}
