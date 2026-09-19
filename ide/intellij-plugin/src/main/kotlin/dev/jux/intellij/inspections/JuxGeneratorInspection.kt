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
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * The compiler's generator rules (Missing-defs §M.2), checked as you type so
 * they show before a build:
 *
 * - E0990: `yield` that belongs to no generator: in a lambda, a constructor,
 *   a field or property initializer, or a switch expression's arm (Java's
 *   arm-value `yield`; Jux writes the value as the arm's expression).
 * - E0996: a generator whose return type is not `Iterator<T>` (`Stream<T>`
 *   when `async`), reported once at the name. Fix: change the return type.
 * - E0994: `return` with a value inside a generator. Fix: `yield` it instead.
 * - E0995: an interface's default (non-static) method that yields.
 * - E0997: `yield` inside an `unsafe { }` block.
 *
 * Every check reads only the PSI of the function it is in, so it is cheap
 * enough to run on every keystroke.
 */
class JuxGeneratorInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                when (element.elementType) {
                    T.YIELD_KW -> checkYield(element, holder)
                    E.METHOD_DECLARATION -> checkGenerator(element, holder)
                    E.RETURN_STATEMENT -> checkReturn(element, holder)
                }
            }
        }

    /** E0990 and E0997 on one `yield`. */
    private fun checkYield(kw: PsiElement, holder: ProblemsHolder) {
        val fn = JuxGenerators.owner(kw)
        val where = when (fn?.elementType) {
            null -> "outside a function"
            E.LAMBDA_EXPRESSION -> "in a lambda"
            E.CONSTRUCTOR_DECLARATION -> "in a constructor"
            E.OPERATOR_DECLARATION -> "in an operator"
            E.METHOD_DECLARATION -> null
            else -> "in an initializer"
        }
        if (where != null) {
            holder.registerProblem(kw, "'yield' $where: only a named function or method can be a generator (E0990)",
                ProblemHighlightType.GENERIC_ERROR)
            return
        }
        if (JuxGenerators.insideBefore(kw, fn!!, E.SWITCH_EXPRESSION)) {
            holder.registerProblem(kw,
                "'yield' in a switch expression's arm: write the value as the arm's expression, `case 1 -> 10;` (E0990)",
                ProblemHighlightType.GENERIC_ERROR)
            return
        }
        if (JuxGenerators.insideBefore(kw, fn, E.UNSAFE_STATEMENT)) {
            holder.registerProblem(kw, "'yield' inside an 'unsafe' block (E0997)", ProblemHighlightType.GENERIC_ERROR)
        }
    }

    /** E0996 and E0995 on a generator's declaration. */
    private fun checkGenerator(fn: PsiElement, holder: ProblemsHolder) {
        if (!JuxGenerators.isGenerator(fn)) return
        val anchor = (fn as? JuxNamedElement)?.nameIdentifier ?: return
        val owner = JuxHierarchy.enclosingType(fn)
        if (owner is JuxTypeDeclaration && JuxHierarchy.isInterface(owner) && !JuxHierarchy.hasModifier(fn, "static")) {
            holder.registerProblem(anchor,
                "An interface's default method cannot be a generator: its iterator would outlive the object (E0995)",
                ProblemHighlightType.GENERIC_ERROR)
        }
        if (!JuxGenerators.returnsSequence(fn)) {
            val expected = JuxGenerators.expectedReturnName(fn)
            val wanted = "$expected<${elementTypeText(fn)}>"
            holder.registerProblem(anchor,
                "A generator returns '$expected<T>'${if (expected == "Stream") " when async" else ""} (E0996)",
                ProblemHighlightType.GENERIC_ERROR, ChangeReturnTypeFix(fn, wanted))
        }
    }

    /** E0994: `return value;` inside a generator. */
    private fun checkReturn(ret: PsiElement, holder: ProblemsHolder) {
        val value = ret.children.firstOrNull { JuxTypeEngine.isExpression(it) } ?: return
        val fn = JuxGenerators.owner(ret) ?: return
        if (!JuxGenerators.isGenerator(fn)) return
        val kw = ret.node.findChildByType(T.RETURN_KW)?.psi ?: return
        holder.registerProblem(kw,
            "A generator cannot return a value: 'yield' it, or end with a bare 'return;' (E0994)",
            ProblemHighlightType.GENERIC_ERROR, YieldInsteadFix(ret, value.text))
    }

    /**
     * The `T` of the return type a generator should have: the current return
     * type when it is a plain value type (`int count()` becomes
     * `Iterator<int>`), otherwise the type of the first value it yields.
     */
    private fun elementTypeText(fn: PsiElement): String {
        val declared = JuxHierarchy.returnTypeText(fn)
        if (!declared.isNullOrEmpty() && declared != "void") return declared
        val firstYield = JuxGenerators.ownYields(fn).firstOrNull()
        val value = firstYield?.parent?.children?.firstOrNull { JuxTypeEngine.isExpression(it) }
        val t = JuxTypeEngine.typeOf(value)
        return if (t is JuxType.Unknown) "int" else t.presentable()
    }

    /** Replaces the function's return type with [wanted] (`Iterator<int>`). */
    private class ChangeReturnTypeFix(fn: PsiElement, private val wanted: String) :
        LocalQuickFixAndIntentionActionOnPsiElement(fn) {
        override fun getText(): String = "Change return type to '$wanted'"
        override fun getFamilyName(): String = "Change generator return type"
        override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
            val ref = startElement.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return
            ref.replace(JuxElementFactory.createTypeReference(project, wanted))
        }
    }

    /** `return x;` becomes `yield x;`: the generator produces the value and carries on. */
    private class YieldInsteadFix(ret: PsiElement, private val valueText: String) :
        LocalQuickFixAndIntentionActionOnPsiElement(ret) {
        override fun getText(): String = "Replace with 'yield $valueText;'"
        override fun getFamilyName(): String = "Replace 'return' with 'yield'"
        override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
            startElement.replace(JuxElementFactory.createStatement(project, "yield $valueText;"))
        }
    }
}
