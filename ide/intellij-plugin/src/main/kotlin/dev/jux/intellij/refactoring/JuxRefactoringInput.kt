package dev.jux.intellij.refactoring

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.Messages
import com.intellij.refactoring.util.CommonRefactoringUtil

/**
 * The questions a refactoring asks the user (a new name, a target package),
 * behind one seam tests can answer.
 *
 * In the IDE each question is a small input dialog. In a headless test there
 * is no one to ask, so [answers] supplies replies by title; an unanswered
 * question takes its default, which is what a user pressing Enter gets.
 */
internal object JuxRefactoringInput {

    /** Test replies, by dialog title. Cleared by the test that sets them. */
    val answers = HashMap<String, String?>()

    /**
     * Ask for a line of text; null when the user cancelled.
     *
     * [validate] rejects an answer with a message; the dialog stays open on a
     * rejected answer, and a test reply that fails it throws.
     */
    fun ask(
        project: Project,
        title: String,
        prompt: String,
        default: String,
        validate: (String) -> String? = { null },
    ): String? {
        if (ApplicationManager.getApplication().isUnitTestMode) {
            val answer = if (answers.containsKey(title)) answers[title] else default
            answer?.let { validate(it) }?.let { throw IllegalArgumentException(it) }
            return answer
        }
        val validator = object : com.intellij.openapi.ui.InputValidatorEx {
            override fun getErrorText(inputString: String): String? = validate(inputString.trim())
            override fun checkInput(inputString: String): Boolean = validate(inputString.trim()) == null
            override fun canClose(inputString: String): Boolean = checkInput(inputString)
        }
        return Messages.showInputDialog(project, prompt, title, null, default, validator)?.trim()
    }

    /** Refuse a refactoring with [message]; nothing has been changed. */
    fun refuse(project: Project, editor: Editor?, title: String, message: String) {
        CommonRefactoringUtil.showErrorHint(project, editor, message, title, null)
    }

    /** The rule every generated name has to follow: a Jux identifier that is not a keyword. */
    fun identifierProblem(name: String): String? = when {
        name.isEmpty() -> "Enter a name."
        !name[0].isJavaIdentifierStart() || !name.all { it.isJavaIdentifierPart() } -> "`$name` is not a valid identifier."
        dev.jux.intellij.highlight.JuxTokenTypes.keywordType(name) != null -> "`$name` is a keyword."
        else -> null
    }
}
