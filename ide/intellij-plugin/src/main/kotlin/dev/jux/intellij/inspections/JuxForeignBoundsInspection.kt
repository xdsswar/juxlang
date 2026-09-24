package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxOperators
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * **E0446** for a bound a FOREIGN method puts on its own type's parameters
 * (Bindgen §G.6.4.4, ERRATA E77).
 *
 * `Vec::sort` is `where T: Ord` in Rust, and the generated `rust.std` stub
 * carries that as `@RustBounds("T: Ord")` on the member. Without this check
 * `people.sort()` on a `Vec<Person>` whose `Person` declares no
 * `operator<=>` compiles in the editor's eyes and then dies in rustc as
 * E0277 ("the trait bound `Person: Ord` is not satisfied"), exactly the leak
 * the Jux-level diagnostics exist to prevent.
 *
 * What each Rust trait asks of a Jux type is the operator rule the backend
 * follows (LANG-V1 §7.14.4): `Ord` and `PartialOrd` are `operator<=>`, `Hash`
 * is `operator hash`. The difference between the two orderings is the float
 * case: a `double` element meets `PartialOrd` but has no TOTAL order, because
 * NaN compares with nothing. An array or a collection element meets neither
 * (both are shared handles whose contents change under them), and a foreign
 * element type answers for itself.
 *
 * Silent whenever the answer is not certain: an unresolved receiver, a type
 * parameter (checked where it is instantiated), a bound naming a trait this
 * rule says nothing about (`Clone`, `Copy`, `Borrow`, `Default`). A false
 * E0446 would paint red over code that builds.
 */
class JuxForeignBoundsInspection : LocalInspectionTool() {

    /** `@RustBounds("T: Ord, A: Clone")` as the stub emitter writes it. */
    private val BOUNDS = Regex("""@RustBounds\s*\(\s*"([^"]*)"\s*\)""", RegexOption.IGNORE_CASE)

    /** The floating-point primitives: ordered, but not TOTALLY ordered. */
    private val FLOATS = setOf("float", "double", "f32", "f64")

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.CALL_EXPRESSION) return
                checkCall(element, holder)
            }
        }

    private fun checkCall(call: PsiElement, holder: ProblemsHolder) {
        val callee = call.firstChild ?: return
        if (callee.elementType !== E.FIELD_ACCESS_EXPRESSION) return
        val member = JuxTypeEngine.resolveMemberAccess(callee, JuxTypeEngine.argumentCount(call)) ?: return
        val method = member.element
        if (method.elementType !== E.METHOD_DECLARATION) return
        // Only a generated stub declares `@RustBounds`, and only a foreign
        // type's parameters are what the bound talks about.
        if (!isStub(member.owner.decl)) return
        val name = (method as? dev.jux.intellij.psi.JuxNamedElement)?.name ?: return

        val params = typeParameterNames(member.owner.decl)
        val bare = member.owner.decl.name ?: return
        for (bound in boundsOf(method)) {
            // `T: Ord` -- the parameter the bound names, and the Rust trait it
            // asks of it. Anything without the colon is not a bound.
            val colon = bound.indexOf(':')
            if (colon < 0) continue
            val param = bound.substring(0, colon).trim()
            val rustTrait = bound.substring(colon + 1).trim()
            val index = params.indexOf(param).takeIf { it >= 0 } ?: continue
            val argument = member.owner.args.getOrNull(index) ?: continue
            val (why, needs) = when (rustTrait) {
                "Ord", "PartialOrd" ->
                    (orderBlocker(argument, total = rustTrait == "Ord") ?: continue) to "to be ordered"
                "Hash" -> (hashBlocker(argument) ?: continue) to "to have `operator hash`"
                else -> continue
            }
            holder.registerProblem(
                call,
                "`$name` needs the elements of `$bare<${argument.presentable()}>` $needs, and $why (E0446)",
                ProblemHighlightType.GENERIC_ERROR,
            )
            // The compiler reports the FIRST unmet bound and stops, so one
            // call never grows a column of overlapping errors.
            return
        }
    }

    /** Every `P: Trait` a method's `@RustBounds` declares, in written order. */
    private fun boundsOf(method: PsiElement): List<String> {
        // An annotation sits either directly on the declaration or inside its
        // modifier list, depending on how the declaration was written.
        val holders = listOfNotNull(method, method.node.findChildByType(E.MODIFIER_LIST)?.psi)
        for (holder in holders) {
            for (child in holder.children) {
                if (child.elementType !== E.ANNOTATION) continue
                val match = BOUNDS.find(child.text) ?: continue
                return match.groupValues[1].split(',').map { it.trim() }.filter { it.isNotEmpty() }
            }
        }
        return emptyList()
    }

    /** The declared type-parameter names of [decl], in declaration order. */
    private fun typeParameterNames(decl: JuxTypeDeclaration): List<String> =
        decl.node.findChildByType(E.TYPE_PARAMETER_LIST)?.psi?.children
            ?.filter { it.elementType === E.TYPE_PARAMETER }
            ?.mapNotNull { (it as? dev.jux.intellij.psi.JuxNamedElement)?.name }
            ?: emptyList()

    /** True when [decl] comes from a generated `.jux.d` stub, i.e. it is foreign. */
    private fun isStub(decl: JuxTypeDeclaration): Boolean =
        decl.containingFile?.name?.endsWith(".jux.d") == true

    /**
     * Why values of [type] cannot be ORDERED, or null when they can. [total]
     * asks for Rust's `Ord` (what `sort` and `binary_search` need); otherwise
     * `PartialOrd`. Mirrors the checker's `order_blocker`, wording included.
     */
    private fun orderBlocker(type: JuxType, total: Boolean): String? = when (type) {
        is JuxType.Nullable -> orderBlocker(type.inner, total)
        is JuxType.Primitive ->
            if (total && type.name in FLOATS) "a `${type.name}` has no total order (NaN compares with nothing)"
            else null
        is JuxType.ArrayType -> "an array has no order"
        is JuxType.FunctionType -> "a function value has no order"
        is JuxType.TupleType -> type.elements.firstNotNullOfOrNull { orderBlocker(it, total) }
        is JuxType.ClassType -> classOrderBlocker(type)
        // A type parameter is checked where it is instantiated; an unknown
        // type is not something to paint an error over.
        else -> null
    }

    /**
     * The named-type half of [orderBlocker]. Nothing DERIVES an order in Jux,
     * so a class, record, struct or enum is ordered exactly when it (or a type
     * it extends) declares `operator<=>`.
     */
    private fun classOrderBlocker(ct: JuxType.ClassType): String? {
        val name = ct.decl.name ?: return null
        if (isStub(ct.decl)) {
            // A collection is a shared handle (§6.5.1), which Rust gives no
            // order. Any other foreign type answers for itself.
            return if (hasAnnotation(ct.decl, "RustCollection")) {
                "a collection has no order (it is a shared handle)"
            } else {
                null
            }
        }
        // `<=>` is inherited, so the whole supertype closure is asked. Being
        // generous here is the safe direction: it can only silence the
        // warning, never invent one.
        if (JuxTypeEngine.typeAndSupertypes(ct).any { JuxOperators.declaredIn(it.decl, "<=>").isNotEmpty() }) {
            return null
        }
        return "`$name` declares no `operator<=>`"
    }

    /**
     * Why values of [type] have no `operator hash`, or null when they do.
     *
     * Deliberately shallower than the checker's `hash_blocker`, which walks a
     * record's components and an enum's payloads to find the one that blocks
     * the derive. Only the cases that are certain from the type alone are
     * reported here: an array, a collection and a function value. A user type
     * derives its hash from its components, and reproducing that walk in the
     * editor would risk the false positive this whole inspection is written
     * to avoid.
     */
    private fun hashBlocker(type: JuxType): String? = when (type) {
        is JuxType.Nullable -> hashBlocker(type.inner)
        is JuxType.ArrayType -> "an array has no hash (it is a shared handle whose elements can change)"
        is JuxType.FunctionType -> "a function value has no hash"
        is JuxType.ClassType ->
            if (isStub(type.decl) && hasAnnotation(type.decl, "RustCollection")) {
                "a collection has no hash (it is a shared handle whose contents can change)"
            } else {
                null
            }
        else -> null
    }

    /** True when [decl] carries the annotation [simpleName] (names match case-insensitively). */
    private fun hasAnnotation(decl: JuxTypeDeclaration, simpleName: String): Boolean {
        val holders = listOfNotNull(decl, decl.node.findChildByType(E.MODIFIER_LIST)?.psi)
        return holders.any { holder ->
            holder.children.any {
                it.elementType === E.ANNOTATION &&
                    it.text.removePrefix("@").substringBefore('(').trim().equals(simpleName, ignoreCase = true)
            }
        }
    }
}
