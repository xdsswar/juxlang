package dev.jux.intellij.psi

import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * The locals a statement declares. A plain `int x = 1;` declares one, the
 * statement itself; a destructuring `var Pt(a, b) = p;` declares each binder
 * inside it. Every scope walk that looks for "the locals of this block" goes
 * through here, so the two shapes can never be treated differently.
 */
object JuxLocals {
    /** The locals [statement] declares, in source order. */
    fun declaredBy(statement: PsiElement): List<PsiElement> = when (statement.elementType) {
        E.LOCAL_VARIABLE -> listOf(statement)
        E.DESTRUCTURING_DECLARATION -> binders(statement)
        else -> emptyList()
    }

    /** Every local declared directly in [block], binders included, in source order. */
    fun blockLocals(block: PsiElement): List<PsiElement> = block.children.flatMap { declaredBy(it) }

    private fun binders(node: PsiElement): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        fun walk(e: PsiElement) {
            for (c in e.children) {
                if (c.elementType === E.LOCAL_VARIABLE) out.add(c) else walk(c)
            }
        }
        walk(node)
        return out
    }
}
