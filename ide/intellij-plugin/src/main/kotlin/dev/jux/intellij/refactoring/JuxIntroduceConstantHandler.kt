package dev.jux.intellij.refactoring

import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.codeStyle.CodeStyleManager
import com.intellij.refactoring.RefactoringActionHandler
import com.intellij.refactoring.rename.inplace.VariableInplaceRenamer
import com.intellij.refactoring.util.CommonRefactoringUtil
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFieldDeclaration
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.resolve.JuxTypeInference

/**
 * Introduce Constant (`Ctrl+Alt+C`) — turn the selected expression into a
 * `private static final` field on the enclosing type and use it in place.
 *
 * The field form is the one the corpus uses
 * (`public static final double TAX_RATE = 0.30;`); `const` and `final` are
 * synonyms in Jux, and `final` is the spelling already in the examples.
 *
 * A field must be given a type, and this refactoring will only write one it can
 * actually read off the source — a literal, a `new T(...)`, or a chain the
 * in-file type inference resolves. When it cannot, it says so and does nothing.
 * A guessed type in a field declaration does not surface as a question, it
 * compiles into a different program.
 */
class JuxIntroduceConstantHandler : RefactoringActionHandler {

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        if (editor == null || file !is JuxFile) return

        val expression = JuxExtractSupport.expressionAt(file, editor)
        if (expression == null) {
            error(project, editor, "Select an expression to extract. An assignment, an increment or a lambda cannot become a constant.")
            return
        }
        val body = JuxExtractSupport.enclosingClassBody(expression)
        if (body == null) {
            error(project, editor, "A constant needs an enclosing type to live in.")
            return
        }
        val typeText = typeOf(expression)
        if (typeText == null) {
            error(project, editor, "Cannot determine the type of this expression. Extract a literal, a `new T(...)`, or a chain whose type resolves in this file.")
            return
        }

        val occurrences = JuxExtractSupport.occurrencesOf(expression, body).ifEmpty { listOf(expression) }
        val name = JuxExtractSupport.uniqueName(DEFAULT_NAME, body)
        val insertAt = fieldInsertionOffset(body)
        val declaration = "private static final $typeText $name = ${expression.text};\n"

        WriteCommandAction.writeCommandAction(project, file)
            .withName("Introduce Constant")
            .run<RuntimeException> {
                val document = editor.document
                // Back to front, so each replacement's offsets are still valid.
                for (occurrence in occurrences.asReversed()) {
                    val range = occurrence.textRange
                    document.replaceString(range.startOffset, range.endOffset, name)
                }
                document.insertString(insertAt, declaration)

                val manager = PsiDocumentManager.getInstance(project)
                manager.commitDocument(document)
                CodeStyleManager.getInstance(project)
                    .reformatText(file, insertAt, insertAt + declaration.length)
                manager.commitDocument(document)
            }

        startRename(project, editor, file, insertAt)
    }

    /** Not reachable: the action is offered in an editor, never over a tree selection. */
    override fun invoke(project: Project, elements: Array<out PsiElement>, dataContext: DataContext?) = Unit

    /**
     * The written type of the expression: read off the source when it is a
     * literal or a `new T(...)`, otherwise resolved through the same in-file
     * inference member completion uses.
     */
    private fun typeOf(expression: PsiElement): String? =
        JuxExtractSupport.inferTypeText(expression)
            ?: JuxTypeInference.resolveReceiverExpression(expression.text, expression)?.type?.name

    /**
     * Where a new field goes: after the last existing field or constant, so the
     * declarations stay grouped; otherwise directly after the body's `{`.
     */
    private fun fieldInsertionOffset(body: PsiElement): Int {
        val lastField = body.children.lastOrNull {
            it.node?.elementType === E.FIELD_DECLARATION || it.node?.elementType === E.CONST_DECLARATION
        }
        if (lastField != null) return lastField.textRange.endOffset + 1
        val brace = body.node.findChildByType(dev.jux.intellij.highlight.JuxTokenTypes.LBRACE)
        return (brace?.textRange?.endOffset ?: body.textRange.startOffset + 1)
    }

    /** Open the in-place rename template on the generated name. */
    private fun startRename(project: Project, editor: Editor, file: PsiFile, insertAt: Int) {
        val field = fieldAt(file, insertAt) ?: return
        val nameOffset = field.nameIdentifier?.textRange?.startOffset ?: return
        editor.caretModel.moveToOffset(nameOffset)
        VariableInplaceRenamer(field, editor).performInplaceRename()
    }

    private fun fieldAt(file: PsiFile, offset: Int): JuxFieldDeclaration? {
        var e: PsiElement? = file.findElementAt(offset)
        while (e != null) {
            if (e is JuxFieldDeclaration) return e
            e = e.parent
        }
        return null
    }

    private fun error(project: Project, editor: Editor, message: String) {
        CommonRefactoringUtil.showErrorHint(project, editor, message, "Introduce Constant", null)
    }

    private companion object {
        /** Starting point for the generated name; suffixed if it is taken. */
        const val DEFAULT_NAME = "CONSTANT"
    }
}
