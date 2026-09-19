package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * E0487 (Async 18.1.9): the time span given to `withTimeout(d, f)` or
 * `Task.delay(d)` is milliseconds (an integer) or a `Duration`.
 *
 * Only a span whose type the editor knows for certain to be something else
 * (a `String`, a floating-point number, a `bool` or a `char`) is reported;
 * anything the type engine cannot pin down is left to the compiler.
 */
class JuxTimeSpanInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.CALL_EXPRESSION) return
                val callee = element.firstChild ?: return
                val name = callee.text.replace(" ", "").substringBefore('<')
                if (name != "withTimeout" && name != "Task.delay") return
                // A user function of the same name is not the built-in.
                if (name == "withTimeout" && callee.elementType === E.REFERENCE_EXPRESSION &&
                    JuxTypeEngine.resolveReferenceExpression(callee) != null
                ) return
                val args = element.node.findChildByType(E.ARGUMENT_LIST)?.psi ?: return
                val span = JuxTypeEngine.expressionChildren(args).firstOrNull() ?: return
                val type = JuxTypeEngine.typeOf(span) as? JuxType.Primitive ?: return
                if (type.name !in NOT_A_SPAN) return
                holder.registerProblem(
                    span,
                    "A time span is milliseconds (an integer) or a `Duration`, found ${type.name} (E0487)",
                    ProblemHighlightType.GENERIC_ERROR,
                )
            }
        }

    private companion object {
        val NOT_A_SPAN = setOf("String", "double", "float", "f32", "f64", "bool", "char")
    }
}
