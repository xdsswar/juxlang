package dev.jux.intellij.psi

import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * The locals a statement declares. A plain `int x = 1;` declares one, the
 * statement itself; a destructuring `var Pt(a, b) = p;` declares each binder
 * inside it. Every scope walk that looks for "the locals of this block" goes
 * through here, so the two shapes can never be treated differently.
 *
 * Patterns bind names too: `case Circle(var r) ->` binds `r` for its arm, and
 * a type test `x => Dog d` binds `d` where the test is known to hold.
 * [bindersInScope] is the one place that says which of those a scope makes
 * visible, for resolution, typing and completion alike.
 */
object JuxLocals {
    /** The locals [statement] declares, in source order. */
    fun declaredBy(statement: PsiElement): List<PsiElement> = when (statement.elementType) {
        E.LOCAL_VARIABLE -> listOf(statement)
        E.DESTRUCTURING_DECLARATION -> binders(statement)
        // `if (!(x => Dog d)) return; d.bark();`: a binder of an `if` guard
        // stays usable after the statement, the early-exit idiom. Being
        // lenient here (the compiler owns the flow rules) costs nothing.
        E.IF_STATEMENT -> firstExpression(statement)?.let { testBinders(it) } ?: emptyList()
        else -> emptyList()
    }

    /** Every local declared directly in [block], binders included, in source order. */
    fun blockLocals(block: PsiElement): List<PsiElement> = block.children.flatMap { declaredBy(it) }

    /**
     * The pattern binders [scope] makes visible to its child [from] (the
     * child of `scope` on the path up from the use):
     *  - a switch arm: every binder of its patterns, in the guard and the body;
     *  - `if` / `while`: the condition's type-test binders, in the branches;
     *  - `a && b`, `a || b`: the binders of `a`, inside `b`;
     *  - `c ? x : y`: the binders of `c`, in either arm.
     */
    fun bindersInScope(scope: PsiElement, from: PsiElement?): List<PsiElement> = when (scope.elementType) {
        E.SWITCH_CASE -> scope.children.filter { it.elementType === E.PATTERN }.flatMap { binders(it) }
        E.IF_STATEMENT, E.WHILE_STATEMENT, E.CONDITIONAL_EXPRESSION -> {
            val cond = firstExpression(scope)
            if (cond == null || cond === from) emptyList() else testBinders(cond)
        }
        E.BINARY_EXPRESSION -> {
            val isLogical = scope.node.findChildByType(T.AND_AND) != null || scope.node.findChildByType(T.OR_OR) != null
            val operands = scope.children
            if (isLogical && operands.size >= 2 && operands[1] === from) testBinders(operands[0]) else emptyList()
        }
        else -> emptyList()
    }

    /**
     * The binders of the type tests inside [expr] (`x => Dog d` binds `d`),
     * not looking into a lambda, whose tests are its own business.
     */
    fun testBinders(expr: PsiElement): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        fun walk(e: PsiElement) {
            if (e.elementType === E.LAMBDA_EXPRESSION || e.elementType === E.SWITCH_EXPRESSION) return
            if (e.elementType === E.LOCAL_VARIABLE && e.parent?.elementType === E.BINARY_EXPRESSION) {
                out.add(e)
                return
            }
            for (c in e.children) walk(c)
        }
        walk(expr)
        return out
    }

    /** Whether [local] is the binder of a type test, `x => Dog d`. */
    fun isTypeTestBinder(local: PsiElement): Boolean =
        local.elementType === E.LOCAL_VARIABLE && local.parent?.elementType === E.BINARY_EXPRESSION

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

    /** The first composite child of [node]: the condition of an `if` or `while`. */
    private fun firstExpression(node: PsiElement): PsiElement? =
        node.children.firstOrNull { it.elementType !== E.CODE_BLOCK && it.elementType !in STATEMENT_NODES }

    /** Statement nodes, never a condition. */
    private val STATEMENT_NODES = setOf(E.EXPRESSION_STATEMENT, E.LOCAL_VARIABLE)
}
