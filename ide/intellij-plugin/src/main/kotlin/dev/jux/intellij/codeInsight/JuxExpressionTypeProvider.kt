package dev.jux.intellij.codeInsight

import com.intellij.lang.ExpressionTypeProvider
import com.intellij.openapi.util.text.StringUtil
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * View | Type Info (Ctrl+Shift+P), as Java has it: the static type of the
 * expression under the caret. When several expressions contain the caret
 * (`a.b().c`), the platform offers each in turn, innermost first, the way
 * Java does.
 *
 * Types come from the plugin's type engine ([JuxTypeEngine]), the same one
 * that drives member completion and go-to, so what this shows is what the
 * editor itself believes. An expression the engine cannot type is left out
 * rather than shown as "unknown".
 */
class JuxExpressionTypeProvider : ExpressionTypeProvider<PsiElement>() {

    override fun getInformationHint(element: PsiElement): String =
        StringUtil.escapeXmlEntities(typeText(element) ?: "unknown")

    override fun getErrorHint(): String = "No expression found"

    override fun getExpressionsAt(elementAt: PsiElement): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        var e: PsiElement? = elementAt
        while (e != null && e !is PsiFile) {
            // An expression's type never leaks past the statement that holds
            // it, and the declaration a statement belongs to is not an
            // expression either.
            if (e.elementType in BOUNDARIES) break
            if (JuxTypeEngine.isExpression(e) && typeText(e) != null) out.add(e)
            e = e.parent
        }
        return out
    }

    /** The presentable type of [e], or null when the engine does not know it. */
    private fun typeText(e: PsiElement): String? {
        // A type name written as a value (`Color` of `Color.Red`) presents
        // as the type itself.
        val type = JuxTypeEngine.typeOf(e)
        return if (type is JuxType.Unknown) null else type.presentable()
    }

    private companion object {
        /** Nodes an expression walk stops at. */
        val BOUNDARIES = setOf(
            E.CODE_BLOCK, E.CLASS_BODY,
            E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION,
        )
    }
}
