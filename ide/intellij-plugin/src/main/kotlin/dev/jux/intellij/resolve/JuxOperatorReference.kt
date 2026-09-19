package dev.jux.intellij.resolve

import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiReferenceBase

/**
 * The operator of `a * b`, `a..b` or `v *= k` as a reference to the
 * `operator` declaration it calls, picked by operand type the way the
 * compiler picks it ([JuxOperators.resolve]).
 *
 * The reference sits on the expression node with its range narrowed to the
 * operator token, so Ctrl+B on the `*` lands on `Vec2 operator*(double k)`
 * while Ctrl+B on either operand still goes to that operand. It is soft: a
 * primitive `+` resolves to nothing and that is not an error.
 */
class JuxOperatorReference(element: PsiElement, range: TextRange) :
    PsiReferenceBase<PsiElement>(element, range, true) {

    override fun resolve(): PsiElement? = JuxOperators.resolve(element)?.element

    /** An operator has no name to rename; a rename elsewhere leaves the use as written. */
    override fun handleElementRename(newElementName: String): PsiElement = element

    override fun getVariants(): Array<Any> = emptyArray()

    companion object {
        /** The operator reference of [element], or null when it is no user-overloadable operator use. */
        fun of(element: PsiElement): JuxOperatorReference? {
            val token = JuxOperators.operatorTokenOf(element) ?: return null
            return JuxOperatorReference(element, TextRange.from(token.startOffsetInParent, token.textLength))
        }
    }
}
