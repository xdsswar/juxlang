package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.SmartPointerManager
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * Java's utility classes, which Jux does not have, reported the way `juxc`
 * reports them (the "Java habits" of JUX-DIAGNOSTICS-ADDENDUM) with the Jux
 * way in the message and, where the rewrite is mechanical, a fix:
 *
 * - `Math.abs(x)` (E0301): numbers carry these as methods. Fix: `x.abs()`,
 *   with Rust's names where they differ (`pow` is `powf`, `log` is `ln`).
 * - `Integer.parseInt(s)`, `Long.parseLong(s)`, `Double.parseDouble(s)`
 *   (E0301). Fix: `s.parse<int>()` and so on.
 * - `Objects.equals(a, b)` / `Objects.requireNonNull(x)` (E0301). Fix:
 *   `a == b` / `x!!`.
 * - `String.valueOf(x)` (E0413). Fix: `$"${x}"`. `String.join` has no
 *   one-line equivalent and gets the message only.
 *
 * A type the program declares under one of these names is not reported: it
 * is the user's `Math`, not Java's, exactly as in the compiler.
 */
class JuxJavaHabitInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.CALL_EXPRESSION) return
                val callee = element.firstChild?.takeIf { it.elementType === E.FIELD_ACCESS_EXPRESSION } ?: return
                val head = JuxTypeEngine.firstExpressionChild(callee)
                    ?.takeIf { it.elementType === E.REFERENCE_EXPRESSION } ?: return
                val cls = head.text.trim()
                val method = JuxTypeEngine.memberName(callee) ?: return
                val args = element.node.findChildByType(E.ARGUMENT_LIST)?.psi
                    ?.let { JuxTypeEngine.expressionChildren(it) } ?: emptyList()
                if (cls == "String") {
                    checkStringStatic(element, head, method, args, holder)
                    return
                }
                val message = UTILITY_CLASSES[cls] ?: return
                // The program's own type of that name wins.
                if (JuxTypeEngine.resolveTypeName(head, cls) != null) return
                holder.registerProblem(
                    head,
                    "Cannot find `$cls` in this scope: $message (E0301)",
                    ProblemHighlightType.GENERIC_ERROR,
                    *listOfNotNull(fixFor(element, cls, method, args)).toTypedArray(),
                )
            }
        }

    private fun checkStringStatic(
        call: PsiElement,
        head: PsiElement,
        method: String,
        args: List<PsiElement>,
        holder: ProblemsHolder,
    ) {
        val (hint, fix) = when (method) {
            "valueOf" -> "a value's text is interpolation, `\$\"\${x}\"`" to
                args.singleOrNull()?.let { ReplaceCallFix(call, "\$\"\${${it.text}}\"", "Replace with interpolation") }
            "join" -> "build the text in a loop, `text += part`, or with interpolation" to null
            "format" -> "format with interpolation, `\$\"\${x}\"`" to null
            else -> return
        }
        holder.registerProblem(
            call,
            "No static method `$method` on `String`: $hint (E0413)",
            ProblemHighlightType.GENERIC_ERROR,
            *listOfNotNull(fix).toTypedArray(),
        )
    }

    /** The mechanical rewrite for `cls.method(args)`, or null when there is none. */
    private fun fixFor(call: PsiElement, cls: String, method: String, args: List<PsiElement>): LocalQuickFix? {
        when (cls) {
            "Math" -> {
                val first = args.firstOrNull() ?: return null
                val rustName = MATH_RENAMES[method] ?: method.takeIf { it in MATH_METHODS } ?: return null
                val rest = args.drop(1).joinToString(", ") { it.text }
                return ReplaceCallFix(call, "${receiverText(first)}.$rustName($rest)", "Replace with '.$rustName()'")
            }
            "Integer", "Long", "Double", "Float", "Short", "Byte" -> {
                val target = PARSERS["$cls.$method"] ?: return null
                val s = args.singleOrNull() ?: return null
                return ReplaceCallFix(call, "${receiverText(s)}.parse<$target>()", "Replace with '.parse<$target>()'")
            }
            "Objects" -> return when {
                method == "equals" && args.size == 2 ->
                    ReplaceCallFix(call, "${args[0].text} == ${args[1].text}", "Replace with '=='")
                method == "requireNonNull" && args.isNotEmpty() ->
                    ReplaceCallFix(call, "${receiverText(args[0])}!!", "Replace with '!!'")
                else -> null
            }
        }
        return null
    }

    /** [e]'s text, parenthesized unless it already reads as one operand. */
    private fun receiverText(e: PsiElement): String {
        val simple = e.elementType in SIMPLE_OPERANDS && !e.text.startsWith("-")
        return if (simple) e.text else "(${e.text})"
    }

    /** Replaces the whole call with [replacement]. */
    private class ReplaceCallFix(call: PsiElement, private val replacement: String, private val label: String) : LocalQuickFix {
        private val pointer = SmartPointerManager.createPointer(call)

        override fun getName(): String = label

        override fun getFamilyName(): String = "Replace Java library call"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val call = pointer.element ?: return
            val file = call.containingFile ?: return
            val docs = PsiDocumentManager.getInstance(project)
            val doc = docs.getDocument(file) ?: return
            doc.replaceString(call.textRange.startOffset, call.textRange.endOffset, replacement)
            docs.commitDocument(doc)
        }
    }

    private companion object {
        /** Java classes Jux has no counterpart for, with the Jux way (the compiler's wording). */
        val UTILITY_CLASSES = mapOf(
            "Math" to "Jux has no `Math` class: numbers carry these as methods, `x.abs()`, `x.sqrt()`, " +
                "`x.powf(y)`, `a.max(b)` on a double",
            "Integer" to "Jux has no boxed number classes: parse text with `s.parse<int>()` (or `<long>`, `<double>`)",
            "Long" to "Jux has no boxed number classes: parse text with `s.parse<long>()`",
            "Double" to "Jux has no boxed number classes: parse text with `s.parse<double>()`",
            "Objects" to "Jux has no `Objects` class: `Objects.equals(a, b)` is `a == b`, and " +
                "`requireNonNull(x)` is `x!!`",
        )

        /** Java `Math` names whose Rust method differs. */
        val MATH_RENAMES = mapOf("pow" to "powf", "log" to "ln")

        /** Java `Math` names that are the same method on a Rust number. */
        val MATH_METHODS = setOf(
            "abs", "sqrt", "cbrt", "floor", "ceil", "round", "signum", "max", "min",
            "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "exp", "log10", "hypot",
        )

        /** `Integer.parseInt` and friends, to the type `parse<T>()` takes. */
        val PARSERS = mapOf(
            "Integer.parseInt" to "int", "Long.parseLong" to "long", "Double.parseDouble" to "double",
            "Float.parseFloat" to "float", "Short.parseShort" to "short", "Byte.parseByte" to "byte",
        )

        /** Operand forms that need no parentheses before a `.method()`. */
        val SIMPLE_OPERANDS = setOf(
            E.REFERENCE_EXPRESSION, E.FIELD_ACCESS_EXPRESSION, E.CALL_EXPRESSION,
            E.PARENTHESIZED_EXPRESSION, E.LITERAL_EXPRESSION, E.INDEX_EXPRESSION,
        )
    }
}
