package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFixAndIntentionActionOnPsiElement
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxOperators

/**
 * E0936, the rules for operators an interface declares (LANG-V1 §7.14.6):
 * an interface may require an operator as a contract, `T operator+(T other);`,
 * and every implementing type declares it. Such an operator:
 *
 * - has no body (fix: remove the body);
 * - takes exactly one operand;
 * - is a binary arithmetic or bitwise operator (`+ - * / % & | ^ << >>`):
 *   equality, ordering, `string`, indexing and the unary forms are rejected.
 *
 * The other side of the rule: an operator with no body outside an interface
 * (and not `= delete`) is E0936 as well.
 */
class JuxInterfaceOperatorInspection : LocalInspectionTool() {

    /** The operator symbols an interface may require. */
    private val CONTRACT_SYMBOLS = setOf("+", "-", "*", "/", "%", "&", "|", "^", "<<", ">>")

    /** `= delete;`, a deleted operator: no body, on purpose. */
    private val DELETED = Regex("=\\s*delete\\b")

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.OPERATOR_DECLARATION) return
                val anchor = JuxOperators.anchorOf(element) ?: return
                val symbol = JuxOperators.symbolOf(element) ?: return
                val owner = JuxHierarchy.enclosingType(element)
                val inInterface = owner != null && JuxHierarchy.isInterface(owner)
                val hasBody = JuxHierarchy.hasBody(element)
                if (!inInterface) {
                    // An operator in a class or at the top level needs its body,
                    // unless it is `= delete` or comes from a generated library
                    // stub (`.jux.d`), which declares signatures only.
                    val deleted = DELETED.containsMatchIn(element.text)
                    val stub = element.containingFile?.name?.endsWith(".jux.d") == true
                    if (!hasBody && !deleted && !stub) {
                        holder.registerProblem(anchor,
                            "'operator$symbol' needs a body: only an interface declares an operator without one (E0936)",
                            ProblemHighlightType.GENERIC_ERROR)
                    }
                    return
                }
                when {
                    symbol !in CONTRACT_SYMBOLS -> holder.registerProblem(anchor,
                        "An interface can only require a binary arithmetic or bitwise operator, not 'operator$symbol' (E0936)",
                        ProblemHighlightType.GENERIC_ERROR)
                    JuxHierarchy.arity(element) != 1 -> holder.registerProblem(anchor,
                        "An interface operator takes exactly one operand (E0936)",
                        ProblemHighlightType.GENERIC_ERROR)
                    hasBody -> holder.registerProblem(anchor,
                        "An interface operator has no body: each implementing type declares its own (E0936)",
                        ProblemHighlightType.GENERIC_ERROR, RemoveBodyFix(element))
                }
            }
        }

    /** Replaces the operator's `{ ... }` with `;`. */
    private class RemoveBodyFix(op: PsiElement) : LocalQuickFixAndIntentionActionOnPsiElement(op) {
        override fun getText(): String = "Remove the operator's body"
        override fun getFamilyName(): String = "Remove operator body"
        override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
            val body = startElement.node.findChildByType(E.CODE_BLOCK)?.psi ?: return
            // Drop the space before the body too, so `operator+(T o) {}` becomes `operator+(T o);`.
            (body.prevSibling as? PsiWhiteSpace)?.delete()
            val semi = JuxElementFactory.createMember(project, "void __f();").lastChild
            body.replace(semi)
        }
    }
}
