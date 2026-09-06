package dev.jux.intellij.codeInsight.surround

import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.codeStyle.CodeStyleManager
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * The shared mechanics behind every Surround With template (`Ctrl+Alt+T`).
 *
 * Surrounding is done as a single **document text edit** followed by a
 * reformat, not by building and splicing PSI. `JuxElementFactory` can only
 * create an identifier, so a PSI route would mean a second, parallel construction
 * grammar to keep in step with the parser; and tree surgery is where language
 * plugins acquire their subtlest corruption bugs. The formatter is already
 * proven idempotent across the whole example corpus, so "write the text, then
 * let the formatter place it" is both shorter and safer.
 */
internal object JuxSurroundSupport {

    /**
     * Replace the range spanned by [elements] with [before] + the original text
     * + [after], reformat the result, and return the range the caret should
     * select — the first occurrence of [placeholder] in the new header, or null
     * when the template has nothing to fill in.
     *
     * The placeholder is located *after* reformatting because reformatting
     * moves offsets; searching the rewritten text is the only way to stay
     * correct without threading a marker through the formatter.
     */
    fun surround(
        project: Project,
        editor: Editor,
        elements: Array<out PsiElement>,
        before: String,
        after: String,
        placeholder: String? = null,
    ): TextRange? {
        val first = elements.firstOrNull() ?: return null
        val start = first.textRange.startOffset
        val end = elements.last().textRange.endOffset
        val file = first.containingFile ?: return null

        val document = editor.document
        val body = document.getText(TextRange(start, end))
        val replacement = before + body + after
        document.replaceString(start, end, replacement)

        val manager = PsiDocumentManager.getInstance(project)
        manager.commitDocument(document)
        CodeStyleManager.getInstance(project).reformatText(file, start, start + replacement.length)
        manager.commitDocument(document)

        if (placeholder == null) return null
        // Search only the region we just wrote, and only its head: the
        // placeholder also being legal body text (`true` inside the surrounded
        // code) must not steal the selection from the header we generated.
        val searchEnd = (start + before.length + placeholder.length + REFORMAT_SLACK)
            .coerceAtMost(document.textLength)
        val at = document.getText(TextRange(start, searchEnd)).indexOf(placeholder)
        if (at < 0) return null
        return TextRange(start + at, start + at + placeholder.length)
    }

    /**
     * Whether every element is a statement — a direct child of a code block (or
     * of the file, in script mode). Surrounding a fragment that is not a whole
     * statement would produce text that no longer parses.
     */
    fun areStatements(elements: Array<out PsiElement>): Boolean =
        elements.isNotEmpty() && elements.all { it.node?.elementType in STATEMENT_KINDS }

    /** Whether the single selected element is an expression. */
    fun isExpression(elements: Array<out PsiElement>): Boolean =
        elements.size == 1 && elements[0].node?.elementType in EXPRESSION_KINDS

    /**
     * Statement kinds that may be surrounded. `EMPTY_STATEMENT` is excluded on
     * purpose: wrapping a stray `;` in an `if` is never what was meant.
     */
    val STATEMENT_KINDS = setOf(
        E.LOCAL_VARIABLE,
        E.EXPRESSION_STATEMENT,
        E.IF_STATEMENT,
        E.WHILE_STATEMENT,
        E.DO_WHILE_STATEMENT,
        E.FOR_STATEMENT,
        E.FOR_EACH_STATEMENT,
        E.SWITCH_STATEMENT,
        E.RETURN_STATEMENT,
        E.BREAK_STATEMENT,
        E.CONTINUE_STATEMENT,
        E.THROW_STATEMENT,
        E.TRY_STATEMENT,
        E.UNSAFE_STATEMENT,
        E.LABELED_STATEMENT,
        E.CODE_BLOCK,
    )

    /** Expression kinds a `(…)` / `!(…)` surrounder accepts. */
    val EXPRESSION_KINDS = setOf(
        E.LITERAL_EXPRESSION,
        E.REFERENCE_EXPRESSION,
        E.PARENTHESIZED_EXPRESSION,
        E.BINARY_EXPRESSION,
        E.UNARY_EXPRESSION,
        E.POSTFIX_EXPRESSION,
        E.CONDITIONAL_EXPRESSION,
        E.RANGE_EXPRESSION,
        E.CALL_EXPRESSION,
        E.INDEX_EXPRESSION,
        E.FIELD_ACCESS_EXPRESSION,
        E.CAST_EXPRESSION,
        E.NEW_EXPRESSION,
        E.SWITCH_EXPRESSION,
        E.THIS_EXPRESSION,
        E.SUPER_EXPRESSION,
    )

    /**
     * How far past the generated header the placeholder search may run.
     * Reformatting only ever adds or removes indentation and line breaks in the
     * header we wrote, so a small constant is enough and keeps a `true` in the
     * surrounded body out of range.
     */
    private const val REFORMAT_SLACK = 16
}
