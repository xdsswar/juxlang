package dev.jux.intellij.refactoring

import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.codeStyle.CodeStyleManager
import com.intellij.refactoring.RefactoringActionHandler
import com.intellij.refactoring.rename.inplace.VariableInplaceRenamer
import com.intellij.refactoring.util.CommonRefactoringUtil
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxLocalVariable

/**
 * Extract Variable (`Ctrl+Alt+V`) — hoist the selected expression into a `var`
 * on its own line and replace it where it appeared.
 *
 * The new binding is declared `var`, so the refactoring never has to infer a
 * type. That is not a shortcut around a hard problem, it is the correct answer:
 * a written-out type here would be a guess that either duplicates what the
 * compiler already knows or is wrong.
 *
 * **Every occurrence is replaced**, and the declaration goes before the first
 * of them. IntelliJ's Java version stops to ask; that dialog is a modal popup
 * with nothing to decide most of the time, and a single Undo reverses the whole
 * thing if the answer was no.
 */
class JuxIntroduceVariableHandler : RefactoringActionHandler {

    override fun invoke(project: com.intellij.openapi.project.Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        if (editor == null || file !is JuxFile) return

        val expression = JuxExtractSupport.expressionAt(file, editor)
        if (expression == null) {
            error(project, editor, "Select an expression to extract. An assignment, an increment or a lambda cannot be hoisted, because that would change when it runs.")
            return
        }
        val block = JuxExtractSupport.enclosingBlock(expression)
        if (block == null) {
            error(project, editor, "There is no enclosing block to declare the variable in.")
            return
        }
        val occurrences = JuxExtractSupport.occurrencesOf(expression, block).ifEmpty { listOf(expression) }
        val anchor = JuxExtractSupport.statementIn(block, occurrences.first())
        if (anchor == null) {
            error(project, editor, "There is no statement to insert the declaration before.")
            return
        }

        val name = JuxExtractSupport.uniqueName(DEFAULT_NAME, block)
        val text = expression.text
        val insertAt = anchor.textRange.startOffset
        val declaration = "var $name = $text;\n"

        WriteCommandAction.writeCommandAction(project, file)
            .withName("Extract Variable")
            .run<RuntimeException> {
                val document = editor.document
                // Replacements run back to front so each one's offsets are still
                // valid when it is applied; the declaration goes in last because
                // it sits before all of them.
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
    override fun invoke(project: com.intellij.openapi.project.Project, elements: Array<out PsiElement>, dataContext: DataContext?) = Unit

    /**
     * Put the caret on the generated name and open the in-place rename
     * template, so the first thing typed names the variable and updates every
     * occurrence at once.
     */
    private fun startRename(project: com.intellij.openapi.project.Project, editor: Editor, file: PsiFile, insertAt: Int) {
        val local = localAt(file, insertAt) ?: return
        val nameOffset = local.nameIdentifier?.textRange?.startOffset ?: return
        editor.caretModel.moveToOffset(nameOffset)
        VariableInplaceRenamer(local, editor).performInplaceRename()
    }

    private fun localAt(file: PsiFile, offset: Int): JuxLocalVariable? {
        var e: PsiElement? = file.findElementAt(offset)
        while (e != null) {
            if (e.node?.elementType === E.LOCAL_VARIABLE) return e as? JuxLocalVariable
            e = e.parent
        }
        return null
    }

    private fun error(project: com.intellij.openapi.project.Project, editor: Editor, message: String) {
        CommonRefactoringUtil.showErrorHint(project, editor, message, "Extract Variable", null)
    }

    private companion object {
        /** Starting point for the generated name; suffixed if it is taken. */
        const val DEFAULT_NAME = "value"
    }
}
