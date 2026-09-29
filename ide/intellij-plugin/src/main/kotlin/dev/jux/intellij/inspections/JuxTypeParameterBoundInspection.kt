package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.DumbService
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.elementType
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.psi.JuxTypeParameter
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * What a type parameter's `extends` bound can mean (JUX-TYPE-SYSTEM-ADDENDUM
 * §T.4.6, ERRATA E135), mirrored with the compiler's wording:
 *
 * - **E0459**, a bound no other type can meet: a Rust type, a record, an
 *   enum, a primitive or `String`. Such a bound admits exactly one type. A
 *   `final` Jux class stays a legal bound, as Java has it.
 * - **E0419**, an intersection naming two classes: a class extends one class,
 *   so nothing is both. The class may stand anywhere in the intersection
 *   (`T extends Named & Base` is legal), so only the COUNT of classes matters.
 *
 * Interfaces are free, in any number and order, and a bound naming another
 * type parameter (`<R extends K>`) is checked where `K` is. Silent on a bound
 * it cannot resolve, and on a bound written with a suffix (`int[]`, `T?`),
 * which the language server reports.
 */
class JuxTypeParameterBoundInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor {
        if (DumbService.isDumb(holder.project)) return PsiElementVisitor.EMPTY_VISITOR
        // A library's own declarations are the library's business.
        if (holder.file.name.endsWith(".jux.d")) return PsiElementVisitor.EMPTY_VISITOR
        return object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element is JuxTypeParameter) check(element, holder)
            }
        }
    }

    private fun check(param: JuxTypeParameter, holder: ProblemsHolder) {
        val name = param.name ?: return
        val classes = ArrayList<Pair<JuxTypeDeclaration, PsiElement>>()
        for (ref in JuxTypeEngine.boundReferences(param)) {
            if (hasSuffix(ref)) continue
            val shown = ref.text.trim()
            val why = when (val bound = JuxTypeEngine.typeOfTypeReference(ref)) {
                is JuxType.Primitive ->
                    if (bound.name == "String" || bound.name == "string") "`$shown` is final"
                    else "`$shown` is a primitive type"
                is JuxType.ClassType -> {
                    val decl = bound.decl
                    when {
                        JuxHierarchy.isRustType(decl) -> "`$shown` is a Rust type, and no Jux type can extend one"
                        decl.name == "String" && JuxTypeIndex.isLibraryRealmPackage(JuxAutoImport.packageOf(decl)) ->
                            "`$shown` is final"
                        decl.elementType === E.RECORD_DECLARATION -> "`$shown` is a record, and a record is final"
                        decl.elementType === E.ENUM_DECLARATION -> "`$shown` is an enum, and an enum is final"
                        JuxHierarchy.isClass(decl) -> { classes.add(decl to ref); null }
                        else -> null
                    }
                }
                else -> null
            } ?: continue
            holder.registerProblem(
                ref,
                "`$name extends $shown` admits only `$shown` itself: $why (§T.4.6) (E0459)",
                ProblemHighlightType.GENERIC_ERROR,
            )
        }
        if (classes.size < 2) return
        val (a, _) = classes[0]
        val (b, at) = classes[1]
        val an = a.name ?: return
        val bn = b.name ?: return
        val message = when {
            JuxHierarchy.inheritsFrom(a, bn) ->
                "`$name` is bounded by two classes, `$an` and `$bn`, and `$an` already extends `$bn`: keep only `$an` (E0419)"
            JuxHierarchy.inheritsFrom(b, an) ->
                "`$name` is bounded by two classes, `$an` and `$bn`, and `$bn` already extends `$an`: keep only `$bn` (E0419)"
            else ->
                "`$name` is bounded by two classes, `$an` and `$bn`, and no type extends both: " +
                    "a class extends exactly one class (§T.4.6) (E0419)"
        }
        holder.registerProblem(at, message, ProblemHighlightType.GENERIC_ERROR)
    }

    /**
     * A bound written with `[]`, `?` or `*` after its name: an array, nullable
     * or pointer type, which the angle-list parser leaves as tokens after the
     * reference. Left to the language server.
     */
    private fun hasSuffix(ref: PsiElement): Boolean {
        var next = ref.nextSibling
        while (next is PsiWhiteSpace) next = next.nextSibling
        return next.elementType === T.LBRACKET || next.elementType === T.QUESTION || next.elementType === T.STAR
    }
}
