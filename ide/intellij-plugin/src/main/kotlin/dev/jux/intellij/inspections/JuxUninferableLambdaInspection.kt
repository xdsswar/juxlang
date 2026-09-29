package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * **E0453** for a `var` lambda whose parameter has no written type and which
 * nothing uses (LANG-V1 §7.9, ERRATA E145), mirrored with the compiler's
 * wording.
 *
 * A `var` lambda has the function type its parameters and body give it. A
 * parameter written without a type takes one from a use: a call
 * (`add(1, 2)`), or a function-typed slot the lambda is passed to. A lambda
 * nothing ever names has no use to take it from:
 *
 * ```
 * var unused = (x) -> x + 1;   // E0453: the type of `x` is never given
 * ```
 *
 * The checker's rule is by name: the lambda counts as used when its name is
 * read anywhere in the enclosing function. This reads it the same way, so a
 * name used even once (or shadowed and used) is left alone.
 */
class JuxUninferableLambdaInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor {
        if (holder.file.name.endsWith(".jux.d")) return PsiElementVisitor.EMPTY_VISITOR
        return object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType === E.LOCAL_VARIABLE) check(element, holder)
            }
        }
    }

    private fun check(local: PsiElement, holder: ProblemsHolder) {
        // `var f = ...`: a written type gives the lambda its parameters' types.
        if (local.node.findChildByType(T.VAR_KW) == null) return
        if (local.node.findChildByType(E.TYPE_REFERENCE) != null) return
        val name = (local as? JuxNamedElement)?.name ?: return
        val lambda = initializer(local)?.takeIf { it.elementType === E.LAMBDA_EXPRESSION } ?: return
        val untyped = JuxTypeEngine.lambdaParameters(lambda)
            .firstOrNull { it.node.findChildByType(E.TYPE_REFERENCE) == null } ?: return
        val param = (untyped as? JuxNamedElement)?.name ?: return
        if (isNamedIn(enclosingFunction(local), name)) return
        holder.registerProblem(
            untyped,
            "cannot infer the type of `$param`, a parameter of the lambda `$name`: the lambda is never " +
                "called or passed anywhere that would give it one (E0453)",
            ProblemHighlightType.GENERIC_ERROR,
        )
    }

    /** The expression after the declaration's `=`. */
    private fun initializer(local: PsiElement): PsiElement? {
        var sawEq = false
        var c: PsiElement? = local.firstChild
        while (c != null) {
            if (c.elementType === T.EQ) sawEq = true
            else if (sawEq && JuxTypeEngine.isExpression(c)) return c
            c = c.nextSibling
        }
        return null
    }

    /** The method, constructor or operator the declaration is in, else its file (a script's top level). */
    private fun enclosingFunction(local: PsiElement): PsiElement =
        generateSequence(local.parent) { it.parent }.firstOrNull {
            it.elementType === E.METHOD_DECLARATION || it.elementType === E.CONSTRUCTOR_DECLARATION ||
                it.elementType === E.OPERATOR_DECLARATION || it is PsiFile
        } ?: local.containingFile

    /** Whether a bare [name] is read anywhere in [scope], the lambda's own body included. */
    private fun isNamedIn(scope: PsiElement, name: String): Boolean =
        PsiTreeUtil.findChildrenOfType(scope, PsiElement::class.java).any {
            it.elementType === E.REFERENCE_EXPRESSION && JuxTypeEngine.memberName(it) == name
        }
}
