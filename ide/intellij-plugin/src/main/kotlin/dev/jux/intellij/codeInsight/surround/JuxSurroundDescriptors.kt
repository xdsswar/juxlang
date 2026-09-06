package dev.jux.intellij.codeInsight.surround

import com.intellij.lang.surroundWith.SurroundDescriptor
import com.intellij.lang.surroundWith.Surrounder
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Surround With (`Ctrl+Alt+T`) over whole statements — the Java templates,
 * spelled in Jux.
 *
 * A template that needs a condition writes `true` and selects it, so the popup
 * closes with the caret already on the part you have to replace. The `for`
 * template uses the for-each form: a C-style header has three blanks to fill
 * and no way to select more than one of them.
 */
class JuxStatementSurroundDescriptor : SurroundDescriptor {

    override fun getElementsToSurround(file: PsiFile, startOffset: Int, endOffset: Int): Array<PsiElement> {
        val statements = statementsIn(file, startOffset, endOffset)
        return statements.toTypedArray()
    }

    override fun getSurrounders(): Array<Surrounder> = SURROUNDERS

    /** Statement templates never exclude the expression ones from the popup. */
    override fun isExclusive(): Boolean = false

    /**
     * The consecutive statements fully covered by `[startOffset, endOffset)`.
     *
     * A statement is a direct child of a code block, or of the file itself in
     * script mode (`JuxParser` puts top-level statements straight under the
     * file). Anchoring on the parent rather than on offsets is what stops a
     * selection that clips half an `if` from producing unparseable output.
     */
    private fun statementsIn(file: PsiFile, startOffset: Int, endOffset: Int): List<PsiElement> {
        val start = file.findElementAt(startOffset) ?: return emptyList()
        val anchor = statementAncestor(start) ?: return emptyList()
        // A caret with no selection surrounds the one statement it sits in.
        if (endOffset <= startOffset) return listOf(anchor)

        val out = ArrayList<PsiElement>()
        var current: PsiElement? = anchor
        while (current != null && current.textRange.startOffset < endOffset) {
            if (current !is PsiWhiteSpace && current.node?.elementType in JuxSurroundSupport.STATEMENT_KINDS) {
                if (current.textRange.endOffset > endOffset) return emptyList()
                out.add(current)
            }
            current = current.nextSibling
        }
        return out
    }

    /** Walk out to the nearest element that is a statement in a block. */
    private fun statementAncestor(from: PsiElement): PsiElement? {
        var e: PsiElement? = from
        while (e != null) {
            val parentType = e.parent?.node?.elementType
            val isStatement = e.node?.elementType in JuxSurroundSupport.STATEMENT_KINDS
            if (isStatement && (parentType === E.CODE_BLOCK || e.parent is PsiFile)) return e
            e = e.parent
        }
        return null
    }

    private companion object {
        val SURROUNDERS: Array<Surrounder> = arrayOf(
            JuxTemplateSurrounder("if", "if (true) {\n", "\n}\n", "true"),
            JuxTemplateSurrounder("if / else", "if (true) {\n", "\n} else {\n}\n", "true"),
            JuxTemplateSurrounder("while", "while (true) {\n", "\n}\n", "true"),
            JuxTemplateSurrounder("do / while", "do {\n", "\n} while (true);\n", "true"),
            JuxTemplateSurrounder("for", "for (var item : items) {\n", "\n}\n", "items"),
            JuxTemplateSurrounder("try / catch", "try {\n", "\n} catch (Exception e) {\n}\n", "Exception"),
            JuxTemplateSurrounder("try / finally", "try {\n", "\n} finally {\n}\n"),
            JuxTemplateSurrounder(
                "try / catch / finally",
                "try {\n",
                "\n} catch (Exception e) {\n} finally {\n}\n",
                "Exception",
            ),
            JuxTemplateSurrounder("{ } block", "{\n", "\n}\n"),
            JuxTemplateSurrounder("unsafe { }", "unsafe {\n", "\n}\n"),
        )
    }
}

/**
 * Surround With over a single expression: `(expr)` to force precedence, and
 * `!(expr)` to negate a condition.
 *
 * Both are one-keystroke fixes for things you notice mid-line, which is why
 * they are worth a template even though they are short enough to type.
 */
class JuxExpressionSurroundDescriptor : SurroundDescriptor {

    override fun getElementsToSurround(file: PsiFile, startOffset: Int, endOffset: Int): Array<PsiElement> {
        if (endOffset <= startOffset) return PsiElement.EMPTY_ARRAY
        val start = file.findElementAt(startOffset) ?: return PsiElement.EMPTY_ARRAY
        var e: PsiElement? = start
        while (e != null) {
            val range = e.textRange
            if (range.startOffset == startOffset && range.endOffset == endOffset &&
                e.node?.elementType in JuxSurroundSupport.EXPRESSION_KINDS
            ) {
                return arrayOf(e)
            }
            if (range.endOffset > endOffset && range.startOffset < startOffset) break
            e = e.parent
        }
        return PsiElement.EMPTY_ARRAY
    }

    override fun getSurrounders(): Array<Surrounder> = SURROUNDERS

    override fun isExclusive(): Boolean = false

    private companion object {
        val SURROUNDERS: Array<Surrounder> = arrayOf(
            JuxExpressionSurrounder("(expr)", "(", ")"),
            JuxExpressionSurrounder("!(expr)", "!(", ")"),
        )
    }
}
