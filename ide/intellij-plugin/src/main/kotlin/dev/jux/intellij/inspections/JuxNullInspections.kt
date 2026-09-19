package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * The written type of a local, parameter or field, or null when it has none
 * (`var`, a lambda parameter without a type).
 */
internal fun writtenTypeText(decl: PsiElement): String? =
    decl.node.findChildByType(E.TYPE_REFERENCE)?.text?.filterNot { it.isWhitespace() }

/**
 * "Condition is always true/false": `x == null` or `x != null` where `x` is
 * declared with a type that cannot hold `null`.
 *
 * In Jux a type is non-null unless it says `?` (Type system §T.6), so a
 * variable declared `String s` or `int n` never holds `null` and the test
 * always answers the same. Only a written type counts (a `var`'s type is
 * left to the compiler), and a type parameter, `any`, `Option` or an unknown
 * type is left alone: those may stand for a nullable type.
 *
 * The fix writes the answer, and the constant-condition inspection then
 * offers to fold the branch it decides.
 */
class JuxRedundantNullCheckInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.BINARY_EXPRESSION) return
                val op = JuxCodeFacts.binaryOperator(element)?.elementType ?: return
                if (op !== T.EQ_EQ && op !== T.NOT_EQ) return
                val operands = JuxCodeFacts.operands(element).takeIf { it.size == 2 } ?: return
                val (value, nullSide) = when {
                    isNull(operands[1]) -> operands[0] to operands[1]
                    isNull(operands[0]) -> operands[1] to operands[0]
                    else -> return
                }
                if (nullSide === value) return
                val plain = JuxCodeFacts.stripParens(value) ?: return
                if (plain.elementType !== E.REFERENCE_EXPRESSION) return
                val decl = storageOf(plain) ?: return
                if (!neverNull(decl)) return
                val answer = op === T.NOT_EQ
                holder.registerProblem(
                    element,
                    "Condition '${element.text}' is always $answer: '${plain.text}' is not nullable",
                    ReplaceWithAnswerFix(answer),
                )
            }
        }

    private fun isNull(e: PsiElement): Boolean = JuxCodeFacts.stripParens(e)?.text == "null"

    /** Declared with a written, non-nullable, concrete type. */
    private fun neverNull(decl: PsiElement): Boolean {
        val text = writtenTypeText(decl) ?: return false
        if (text.endsWith("?")) return false
        // A raw pointer `T*` and a function pointer `fn(...)` may be null
        // (Layout-ABI §L): comparing one with `null` is how it is tested.
        if (text.contains('*') || text.startsWith("fn(")) return false
        val bare = text.substringBefore('<')
        if (bare in OPEN_TYPES) return false
        val type = JuxTypeEngine.declaredType(decl)
        return when (type) {
            is dev.jux.intellij.resolve.JuxType.ClassType,
            is dev.jux.intellij.resolve.JuxType.ArrayType,
            is dev.jux.intellij.resolve.JuxType.Primitive -> true
            else -> false
        }
    }

    private class ReplaceWithAnswerFix(private val answer: Boolean) : LocalQuickFix {
        override fun getFamilyName(): String = "Replace with '$answer'"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val e = descriptor.psiElement ?: return
            JuxCodeFacts.edit(project, e.containingFile, e.textRange.startOffset, e.textRange.endOffset, answer.toString())
        }
    }

    private companion object {
        /** Types that may stand for a nullable one. */
        val OPEN_TYPES = setOf("any", "Option", "Object")
    }
}

/**
 * "Nullable value used without a check": `x.member` where `x` is declared
 * `T?` and nothing in its function ever checks it, mirroring the compiler's
 * rule that a `T?` is used as a `T` only after a check (§T.6).
 *
 * The check is deliberately generous: any `x == null` / `x != null`, a type
 * test `x => T`, `x!!`, `x ?? e`, `x ?: e`, `assert(...)` mentioning `x`, or
 * any assignment to `x` anywhere in the function counts, so the inspection
 * only speaks where the compiler certainly would. The fix writes `!!`
 * (assert non-null), which compiles; the better fix, a real check, is the
 * reader's to write.
 */
class JuxNullableAccessInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.FIELD_ACCESS_EXPRESSION) return
                if (element.node.findChildByType(T.DOT) == null) return
                val receiver = JuxTypeEngine.expressionChildren(element).firstOrNull() ?: return
                if (receiver.elementType !== E.REFERENCE_EXPRESSION) return
                val decl = storageOf(receiver) ?: return
                if (decl.elementType !== E.LOCAL_VARIABLE && decl.elementType !== E.PARAMETER) return
                if (writtenTypeText(decl)?.endsWith("?") != true) return
                val declScope = functionOf(decl) ?: return
                // A check made outside a lambda does not hold inside it (Type
                // system §T.6.4, E0418 in the compiler): for a use inside a
                // lambda nested in the declaring function, only checks inside
                // that lambda count. `var u = maybe;` inside the lambda is a
                // new local with its own scope, so that pattern stays clean.
                val scope = innermostLambdaBetween(receiver, declScope) ?: declScope
                val name = receiver.text
                if (checkedOrReassigned(scope, name, decl)) return
                holder.registerProblem(
                    receiver,
                    "'$name' may be null: check it first (it is declared '${writtenTypeText(decl)}')",
                    AssertNonNullFix(),
                )
            }
        }

    /** The function (or lambda) whose body the declaration's scope is. */
    private fun functionOf(decl: PsiElement): PsiElement? =
        PsiTreeUtil.findFirstParent(decl) { it.elementType in FUNCTIONS }

    /**
     * The innermost lambda that encloses [use] but lies strictly inside
     * [declScope], or null when the use is not inside such a lambda. Every
     * lambda boundary drops refinements (§T.6.4), so a check in an outer
     * lambda does not hold in one nested inside it either.
     */
    private fun innermostLambdaBetween(use: PsiElement, declScope: PsiElement): PsiElement? {
        var p: PsiElement? = use.parent
        while (p != null && p !== declScope) {
            if (p.elementType === E.LAMBDA_EXPRESSION) return p
            p = p.parent
        }
        return null
    }

    private fun checkedOrReassigned(scope: PsiElement, name: String, decl: PsiElement): Boolean {
        var found = false
        PsiTreeUtil.processElements(scope) { e ->
            when (e.elementType) {
                E.BINARY_EXPRESSION -> {
                    val op = JuxCodeFacts.binaryOperator(e)?.elementType
                    if (op in CHECKS && JuxCodeFacts.operands(e).any { mentions(it, name) }) found = true
                }
                E.CONDITIONAL_EXPRESSION, E.ASSIGNMENT_EXPRESSION ->
                    if (JuxTypeEngine.expressionChildren(e).firstOrNull()?.let { mentions(it, name) } == true) found = true
                E.POSTFIX_EXPRESSION, E.UNARY_EXPRESSION ->
                    if (e.node.findChildByType(T.BANG_BANG) != null && JuxTypeEngine.expressionChildren(e).any { mentions(it, name) }) found = true
                E.CALL_EXPRESSION ->
                    if (e.text.startsWith("assert") && PsiTreeUtil.collectElements(e) { it.elementType === T.IDENTIFIER && it.text == name }.isNotEmpty()) found = true
            }
            // `x!!`, `x ?? e` and `x ?: e` may also be plain token runs.
            if (e.elementType === T.IDENTIFIER && e.text == name) {
                val next = PsiTreeUtil.nextVisibleLeaf(e)?.elementType
                if (next in AFTER_NAME_CHECKS) found = true
            }
            !found
        }
        return found
    }

    private fun mentions(e: PsiElement, name: String): Boolean {
        val plain = JuxCodeFacts.stripParens(e) ?: return false
        return plain.elementType === E.REFERENCE_EXPRESSION && plain.text == name
    }

    private class AssertNonNullFix : LocalQuickFix {
        override fun getFamilyName(): String = "Assert non-null with '!!'"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val e = descriptor.psiElement ?: return
            JuxCodeFacts.edit(project, e.containingFile, e.textRange.endOffset, e.textRange.endOffset, "!!")
        }
    }

    private companion object {
        val FUNCTIONS = setOf(
            E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION, E.LAMBDA_EXPRESSION,
            E.PROPERTY_ACCESSOR, E.INIT_BLOCK, E.STATIC_BLOCK,
        )
        val CHECKS = setOf(T.EQ_EQ, T.NOT_EQ, T.STRICT_EQ, T.STRICT_NOT_EQ, T.FAT_ARROW, T.QUESTION_QUESTION, T.QUESTION_COLON)
        val AFTER_NAME_CHECKS = setOf(T.BANG_BANG, T.QUESTION_QUESTION, T.QUESTION_COLON, T.FAT_ARROW, T.EQ_EQ, T.NOT_EQ, T.EQ)
    }
}
