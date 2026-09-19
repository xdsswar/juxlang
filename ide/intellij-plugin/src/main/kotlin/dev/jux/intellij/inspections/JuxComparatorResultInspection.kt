package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * Mirrors the compiler's comparator rule (Operators §O.2.1, E0410): a lambda
 * given where the callee expects a function returning `Ordering`
 * (`sort_unstable_by`, `binary_search_by`, `max_by`, ...) may return an
 * integer, whose sign picks the order, or an `Ordering`. Anything else is
 * reported with the compiler's own message.
 *
 * An integer result is therefore NOT a mismatch here, which is the point of
 * the rule: `v.sort_unstable_by((a, b) -> a.age - b.age)` is fine.
 *
 * Like the compiler, the result is read from an expression body or from the
 * first `return` of a block body, and only a type the editor knows for sure
 * is reported; unknown types and type variables are left alone.
 */
class JuxComparatorResultInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.LAMBDA_EXPRESSION) return
                val expected = JuxTypeEngine.expectedFunctionType(element) ?: return
                if (!isOrdering(expected.ret)) return
                val value = resultExpression(element) ?: return
                val found = JuxTypeEngine.typeOf(value)
                if (fits(found)) return
                holder.registerProblem(
                    value,
                    "a comparator returns an `int` or an `Ordering`, found ${found.presentable()} (E0410)",
                )
            }
        }

    /** The expression a lambda's result comes from: its expression body, or its first `return`. */
    private fun resultExpression(lambda: PsiElement): PsiElement? {
        val block = lambda.node.findChildByType(E.CODE_BLOCK)?.psi
        if (block == null) {
            // `(a, b) -> <expr>`: the expression after the arrow.
            var c = lambda.node.findChildByType(T.ARROW)?.treeNext
            while (c != null && c.psi != null && !JuxTypeEngine.isExpression(c.psi)) c = c.treeNext
            return c?.psi
        }
        return firstReturnValue(block)
    }

    private fun firstReturnValue(scope: PsiElement): PsiElement? {
        for (child in scope.children) {
            when (child.elementType) {
                // A nested lambda's `return` is its own.
                E.LAMBDA_EXPRESSION -> continue
                E.RETURN_STATEMENT -> return JuxTypeEngine.expressionChildren(child).firstOrNull()
            }
            firstReturnValue(child)?.let { return it }
        }
        return null
    }

    private fun isOrdering(t: JuxType): Boolean =
        (t as? JuxType.ClassType)?.decl?.name == "Ordering"

    private fun fits(t: JuxType): Boolean = when (t) {
        is JuxType.Primitive -> t.name in INTEGERS
        is JuxType.ClassType -> t.decl.name == "Ordering"
        is JuxType.Unknown, is JuxType.TypeVar -> true
        else -> false
    }

    private companion object {
        val INTEGERS = setOf("byte", "short", "int", "long", "ubyte", "ushort", "uint", "ulong", "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "isize", "usize")
    }
}
