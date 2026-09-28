package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.DumbService
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.completion.JuxCompletionRanking
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * W0491 mirrored IDE-side (ERRATA E132): a call to a function, method or
 * constructor marked `@Deprecated`, or a `new` of a deprecated type, is shown
 * struck through with the declaration's message, as the compiler words it:
 * "`Panel.show_inside` is deprecated: Renamed to `show`".
 *
 * A crate's `#[deprecated]` items carry the marker in their generated stub
 * (`@Deprecated(message = "...")`), so this is how a program hears of one in
 * the editor; the program's own `@Deprecated` declarations count too. A stub's
 * own text is never checked, as the compiler does not check it.
 */
class JuxDeprecatedUsageInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor {
        val file = holder.file
        if (file.name.endsWith(".jux.d") || DumbService.isDumb(file.project)) return PsiElementVisitor.EMPTY_VISITOR
        return object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                when (element.elementType) {
                    E.CALL_EXPRESSION -> checkCall(element, holder)
                    E.NEW_EXPRESSION -> checkNew(element, holder)
                }
            }
        }
    }

    private fun checkCall(call: PsiElement, holder: ProblemsHolder) {
        val callee = call.firstChild ?: return
        val args = JuxTypeEngine.argumentCount(call)
        val target = when (callee.elementType) {
            E.REFERENCE_EXPRESSION -> JuxTypeEngine.resolveReferenceExpression(callee, args)
            E.FIELD_ACCESS_EXPRESSION -> JuxTypeEngine.resolveMemberAccess(callee, args)?.element
            else -> null
        } ?: return
        if (target.elementType !== E.METHOD_DECLARATION || !JuxCompletionRanking.isDeprecated(target)) return
        val nameLeaf = lastName(callee) ?: return
        val name = (target as? JuxNamedElement)?.name ?: return
        val owner = JuxHierarchy.enclosingType(target)?.name
        report(holder, nameLeaf, if (owner != null) "$owner.$name" else name, target)
    }

    private fun checkNew(expr: PsiElement, holder: ProblemsHolder) {
        val ref = expr.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return
        if (expr.node.findChildByType(T.LBRACKET) != null) return // an array, not a constructor call
        val type = (JuxTypeEngine.typeOfTypeReference(ref) as? JuxType.ClassType)?.decl ?: return
        val args = JuxTypeEngine.argumentCount(expr)
        val ctor = JuxHierarchy.directChildren(type, E.CONSTRUCTOR_DECLARATION)
            .firstOrNull { JuxHierarchy.arity(it) == args }
        val deprecated = ctor?.takeIf { JuxCompletionRanking.isDeprecated(it) }
            ?: type.takeIf { JuxCompletionRanking.isDeprecated(it) }
            ?: return
        val leaf = ref.node.getChildren(null).lastOrNull { it.elementType === T.IDENTIFIER }?.psi ?: ref
        report(holder, leaf, "new ${type.name}", deprecated)
    }

    private fun report(holder: ProblemsHolder, at: PsiElement, what: String, declaration: PsiElement) {
        val message = deprecationMessage(declaration)
        val text = if (message.isNullOrBlank()) "'$what' is deprecated (W0491)" else "'$what' is deprecated: $message (W0491)"
        holder.registerProblem(at, text, ProblemHighlightType.LIKE_DEPRECATED)
    }

    /** The last name of a callee, `show_inside` of `panel.show_inside`. */
    private fun lastName(callee: PsiElement): PsiElement? =
        PsiTreeUtil.getDeepestLast(callee).takeIf { it.elementType === T.IDENTIFIER || T.KEYWORDS.contains(it.elementType) }

    companion object {
        /**
         * The `message` of a declaration's `@Deprecated`, from either spelling:
         * `@Deprecated(message = "Renamed to `show`")` or `@Deprecated("...")`.
         * Null when the annotation carries none.
         */
        fun deprecationMessage(declaration: PsiElement): String? {
            val holders = listOfNotNull(declaration, declaration.node.findChildByType(E.MODIFIER_LIST)?.psi)
            val annotation = holders.flatMap { it.children.toList() }.firstOrNull {
                it.elementType === E.ANNOTATION &&
                    it.text.removePrefix("@").substringBefore('(').trim().equals("deprecated", ignoreCase = true)
            } ?: return null
            return MESSAGE.find(annotation.text)?.groupValues?.get(1)
                ?.replace("\\\"", "\"")?.replace("\\\\", "\\")
        }

        private val MESSAGE = Regex(""""((?:[^"\\]|\\.)*)"""")
    }
}
