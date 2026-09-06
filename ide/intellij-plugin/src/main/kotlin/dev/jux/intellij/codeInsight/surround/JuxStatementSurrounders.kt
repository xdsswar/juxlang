package dev.jux.intellij.codeInsight.surround

import com.intellij.lang.surroundWith.Surrounder
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement

/**
 * One Surround With template over whole statements: write [before] before the
 * selection and [after] after it, then select [placeholder] so the first thing
 * typed replaces the part that needs filling in.
 *
 * Every Jux template is expressible this way, which is why there is one class
 * here rather than ten. The templates themselves are declared in
 * [JuxStatementSurroundDescriptor].
 */
internal class JuxTemplateSurrounder(
    private val description: String,
    private val before: String,
    private val after: String,
    private val placeholder: String? = null,
) : Surrounder {

    override fun getTemplateDescription(): String = description

    override fun isApplicable(elements: Array<out PsiElement>): Boolean =
        JuxSurroundSupport.areStatements(elements)

    override fun surroundElements(
        project: Project,
        editor: Editor,
        elements: Array<out PsiElement>,
    ): TextRange? = JuxSurroundSupport.surround(project, editor, elements, before, after, placeholder)
}

/**
 * The same shape for a single expression: `(expr)` and `!(expr)`.
 *
 * Kept apart from the statement surrounder so the two applicability rules stay
 * separate — an expression template offered over a statement would generate
 * `(int x = 1;)`.
 */
internal class JuxExpressionSurrounder(
    private val description: String,
    private val before: String,
    private val after: String,
) : Surrounder {

    override fun getTemplateDescription(): String = description

    override fun isApplicable(elements: Array<out PsiElement>): Boolean =
        JuxSurroundSupport.isExpression(elements)

    override fun surroundElements(
        project: Project,
        editor: Editor,
        elements: Array<out PsiElement>,
    ): TextRange? = JuxSurroundSupport.surround(project, editor, elements, before, after, null)
}
