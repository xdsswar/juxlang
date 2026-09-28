package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.DumbService
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.psi.JuxTypeParameter
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * The two errors a `type` alias itself can carry (ERRATA E133), mirrored
 * IDE-side:
 *
 * - **E0498**, an alias that stands for itself: `type A = B; type B = A;`
 *   names no type, and each alias in the cycle is reported.
 * - **E0443**, a use of a generic alias with the wrong number of type
 *   arguments: `Dict<String, int>` with `type Dict<V> = HashMap<String, V>;`.
 *   A use with none at all is left to inference, as the compiler leaves it.
 *
 * Everything else about an alias is expansion: the editor reads `Sh` as the
 * `Shape` it stands for wherever a type is named ([JuxTypeEngine.expandAlias]).
 */
class JuxTypeAliasInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor {
        if (DumbService.isDumb(holder.project)) return PsiElementVisitor.EMPTY_VISITOR
        return object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                when (element.elementType) {
                    E.TYPE_ALIAS_DECLARATION -> checkCycle(element as? JuxTypeDeclaration ?: return, holder)
                    E.TYPE_REFERENCE -> checkArity(element, holder)
                }
            }
        }
    }

    private fun checkCycle(alias: JuxTypeDeclaration, holder: ProblemsHolder) {
        val name = alias.name ?: return
        val anchor = alias.nameIdentifier ?: return
        val seen = HashSet<JuxTypeDeclaration>()
        var current: JuxTypeDeclaration = alias
        while (seen.add(current)) {
            val target = JuxTypeEngine.aliasTargetReference(current) ?: return
            val next = headDeclaration(target) ?: return
            if (!JuxTypeEngine.isTypeAlias(next)) return
            if (next === alias) {
                holder.registerProblem(
                    anchor,
                    "Type alias '$name' stands for itself: its target leads back to it through other aliases, " +
                        "so it names no type -- make one of them name a class, record, enum, interface or primitive (E0498)",
                    ProblemHighlightType.GENERIC_ERROR,
                )
                return
            }
            current = next
        }
    }

    private fun checkArity(ref: PsiElement, holder: ProblemsHolder) {
        val given = JuxHierarchy.typeArguments(ref).size
        if (given == 0) return
        val alias = headDeclaration(ref)?.takeIf { JuxTypeEngine.isTypeAlias(it) } ?: return
        val want = JuxHierarchy.typeParameterNames(alias).size
        if (given == want) return
        val plural = if (want == 1) "" else "s"
        val verb = if (given == 1) "was" else "were"
        holder.registerProblem(
            ref,
            "Type alias '${alias.name}' takes $want type argument$plural, but $given $verb supplied (E0443)",
            ProblemHighlightType.GENERIC_ERROR,
        )
    }

    /** The declaration a written type reference's head names, before any alias is expanded. */
    private fun headDeclaration(ref: PsiElement): JuxTypeDeclaration? {
        val written = HEAD.find(ref.text.trim())?.value ?: return null
        val simple = written.substringAfterLast('.')
        val qualifier = written.substringBeforeLast('.', "").ifEmpty { null }
        return when (val found = JuxTypeEngine.resolveTypeName(ref, simple, qualifier)) {
            is JuxTypeParameter -> null
            is JuxTypeDeclaration -> found
            else -> null
        }
    }

    private companion object {
        /** The dotted name a type reference opens with: `a.b.Dict` of `a.b.Dict<int>[]?`. */
        val HEAD = Regex("""^[A-Za-z_][\w.]*""")
    }
}
