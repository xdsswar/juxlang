package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.completion.JuxCompletionRanking
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * Mirrors the compiler's function-type variance check (Type system §T.3.6,
 * E0410): a function value fits a function-typed slot when every parameter
 * the slot passes is one the value accepts (contravariant) and the value's
 * result is one the slot promises (covariant).
 *
 * `(Animal) -> String` into a `(Dog) -> String` slot is fine; `(Dog) -> String`
 * into `(Animal) -> String` is not, since the slot may pass an Animal that is
 * no Dog. Likewise `() -> Dog` fits `() -> Animal` but not the reverse.
 *
 * Only a value whose function type the editor knows completely is judged, and
 * only a certain misfit is reported, with the compiler's message.
 */
class JuxFunctionTypeVarianceInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                when (element.elementType) {
                    E.LOCAL_VARIABLE, E.FIELD_DECLARATION -> {
                        val ref = element.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return
                        val expected = JuxTypeEngine.typeOfTypeReference(ref) as? JuxType.FunctionType ?: return
                        val value = initializerOf(element) ?: return
                        val name = (element as? JuxNamedElement)?.name ?: return
                        report(holder, value, expected, "declaration of `$name`")
                    }
                    E.ASSIGNMENT_EXPRESSION -> {
                        if (element.node.findChildByType(T.EQ) == null) return
                        val parts = JuxTypeEngine.expressionChildren(element)
                        if (parts.size != 2) return
                        val expected = JuxTypeEngine.typeOf(parts[0]) as? JuxType.FunctionType ?: return
                        report(holder, parts[1], expected, "assignment to `${parts[0].text}`")
                    }
                }
            }
        }

    private fun report(holder: ProblemsHolder, value: PsiElement, expected: JuxType.FunctionType, what: String) {
        // A lambda is typed from its slot, so only a function VALUE is judged.
        if (value.elementType === E.LAMBDA_EXPRESSION) return
        val found = JuxTypeEngine.typeOf(value) as? JuxType.FunctionType ?: return
        if (!complete(found) || !complete(expected)) return
        if (JuxCompletionRanking.fit(found, expected) != 2) return
        holder.registerProblem(
            value,
            "type mismatch in $what: expected ${expected.presentable()}, found ${found.presentable()} (E0410)",
        )
    }

    /** Every part of [t] is known, so a misfit is certain, not a guess. */
    private fun complete(t: JuxType): Boolean = when (t) {
        is JuxType.Unknown -> false
        is JuxType.FunctionType -> t.params.all { complete(it) } && complete(t.ret)
        is JuxType.Nullable -> complete(t.inner)
        is JuxType.ArrayType -> complete(t.element)
        is JuxType.ClassType -> t.args.all { complete(it) }
        else -> true
    }

    /** The initializer after `=` in a declaration, or null. */
    private fun initializerOf(decl: PsiElement): PsiElement? {
        var sawEq = false
        var c: PsiElement? = decl.firstChild
        while (c != null) {
            if (c.elementType === T.EQ) sawEq = true
            else if (sawEq && JuxTypeEngine.isExpression(c)) return c
            c = c.nextSibling
        }
        return null
    }
}
