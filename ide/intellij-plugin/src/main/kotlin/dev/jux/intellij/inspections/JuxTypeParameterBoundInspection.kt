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
 * it cannot resolve.
 *
 * A bound written with a suffix is E0459 too: `T extends int[]` is an array
 * type and `T extends Named?` a nullable one, and neither is extended by
 * anything. A pointer suffix is read through, as the checker reads it:
 * `T extends int*` is judged, and shown, as `int`.
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
            val suffix = suffixOf(ref)
            val written = suffix.filter { it.elementType !== T.STAR }
            if (written.isNotEmpty()) {
                // An unresolved base is the unresolved-name check's alone, as
                // the checker validates the bound's name before its shape.
                if (JuxTypeEngine.typeOfTypeReference(ref) is JuxType.Unknown) continue
                val last = written.last()
                val shown = textFrom(ref, last)
                val why = if (last.elementType === T.QUESTION) "`$shown` is a nullable type" else "`$shown` is an array type"
                holder.registerProblem(
                    holder.manager.createProblemDescriptor(
                        ref,
                        last,
                        "`$name extends $shown` admits only `$shown` itself: $why (§T.4.6) (E0459)",
                        ProblemHighlightType.GENERIC_ERROR,
                        holder.isOnTheFly,
                    ),
                )
                continue
            }
            val shown = textFrom(ref, angleEnd(ref))
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
     * The `[]`, `?` and `*` tokens written after a bound's name (and after its
     * type arguments), which the angle-list parser leaves as loose tokens
     * after the reference, in order. `Named<int>[]?` gives `[`, `]`, `?`.
     */
    private fun suffixOf(ref: PsiElement): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        var next = skipSpace(angleEnd(ref).nextSibling)
        while (next != null) {
            when (next.elementType) {
                T.LBRACKET, T.RBRACKET, T.QUESTION, T.STAR -> out.add(next)
                // `T[N]`: a length inside the brackets.
                T.INT_LITERAL, T.IDENTIFIER -> if (out.lastOrNull()?.elementType !== T.LBRACKET) break else out.add(next)
                else -> break
            }
            next = skipSpace(next.nextSibling)
        }
        return out
    }

    /**
     * The last token of a bound's name and type arguments: the reference
     * itself, or the `>` closing the `<..>` written after it, whose tokens the
     * angle-list parser keeps opaque beside the reference.
     */
    private fun angleEnd(ref: PsiElement): PsiElement {
        var next = skipSpace(ref.nextSibling)
        if (next.elementType !== T.LT) return ref
        var depth = 0
        var last: PsiElement = ref
        while (next != null) {
            depth += when (next.elementType) {
                T.LT -> 1
                T.LT_LT -> 2
                T.GT -> -1
                T.GT_GT -> -2
                else -> 0
            }
            last = next
            // A `>>` closing this list and the enclosing one: the enclosing
            // list's `>` is shared, so the bound ends here.
            if (depth <= 0) return last
            next = next.nextSibling
        }
        return last
    }

    private fun skipSpace(e: PsiElement?): PsiElement? {
        var c = e
        while (c is PsiWhiteSpace || c is com.intellij.psi.PsiComment) c = c.nextSibling
        return c
    }

    /** The written text from [first] to [last], siblings, spaced as the checker prints a type (`Pair<int, int>`). */
    private fun textFrom(first: PsiElement, last: PsiElement): String {
        val sb = StringBuilder()
        var c: PsiElement? = first
        while (c != null) {
            if (c !is PsiWhiteSpace) sb.append(c.text)
            if (c === last) break
            c = c.nextSibling
        }
        var text = sb.toString().filterNot { it.isWhitespace() }.replace(",", ", ")
        // `<V extends Vec<int>>`: the one `>>` token closes the parameter list too.
        while (text.endsWith('>') && text.count { it == '>' } > text.count { it == '<' }) text = text.dropLast(1)
        return text
    }
}
