package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.LocalQuickFixAndIntentionActionOnPsiElement
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.PsiErrorElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.quickfix.JuxCreateFromUsage
import dev.jux.intellij.quickfix.JuxMakeMethodVoidFix
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * "Missing return statement", Java's compile error: a method that declares a
 * result can reach the end of its body without returning one.
 *
 * Built on [JuxCodeFacts.canCompleteNormally], which answers "can't finish"
 * whenever it is unsure (a `switch`, a labeled statement, a loop left by a
 * labeled `break`), so a method that does return is never reported. Methods
 * with a `yield` (generators) and bodies still under construction (a parse
 * error inside) are skipped.
 *
 * Fixes, as in Java: add a `return` at the end (with the type's default
 * value when Jux has one, as a template stop), or make the method `void`
 * when no `return` in it carries a value.
 */
class JuxMissingReturnInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.METHOD_DECLARATION && element.elementType !== E.OPERATOR_DECLARATION) return
                val returnType = JuxHierarchy.returnTypeText(element) ?: return
                if (returnType == "void" || returnType.isEmpty()) return
                val body = element.node.findChildByType(E.CODE_BLOCK)?.psi ?: return
                if (PsiTreeUtil.findChildOfType(body, PsiErrorElement::class.java) != null) return
                if (containsYield(body)) return
                if (!JuxCodeFacts.canCompleteNormally(body)) return
                val close = body.lastChild?.takeIf { it.elementType === T.RBRACE } ?: return
                val name = (element as? JuxNamedElement)?.name ?: "operator"
                val fixes = ArrayList<LocalQuickFix>()
                fixes.add(AddReturnFix(element))
                if (element.elementType === E.METHOD_DECLARATION && JuxMakeMethodVoidFix.returnsNoValue(element)) {
                    fixes.add(JuxMakeMethodVoidFix(element, name))
                }
                holder.registerProblem(close, "Missing return statement", ProblemHighlightType.GENERIC_ERROR, *fixes.toTypedArray())
            }
        }

    private fun containsYield(body: PsiElement): Boolean =
        PsiTreeUtil.collectElements(body) { it.elementType === T.YIELD_KW }.isNotEmpty()

    /** Adds `return <default>;` before the method's closing brace, the value as a template stop. */
    private class AddReturnFix(method: PsiElement) : LocalQuickFixAndIntentionActionOnPsiElement(method) {
        override fun getText(): String = "Add 'return' statement"

        override fun getFamilyName(): String = "Add 'return' statement"

        override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
            val body = startElement.node.findChildByType(E.CODE_BLOCK)?.psi ?: return
            val type = JuxHierarchy.returnTypeText(startElement) ?: return
            val value = JuxCodeFacts.defaultValue(type) ?: "value"
            val placed = JuxCodeFacts.addStatementAtEnd(project, body, "return $value;") ?: return
            val valueStop = JuxTypeEngine.firstExpressionChild(placed)
            JuxCreateFromUsage.runTemplate(project, editor, placed, listOfNotNull(valueStop))
        }
    }
}
