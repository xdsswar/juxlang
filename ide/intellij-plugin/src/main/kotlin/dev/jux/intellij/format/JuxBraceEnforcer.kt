package dev.jux.intellij.format

import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.codeStyle.CodeStyleManager
import com.intellij.psi.codeStyle.CodeStyleSettings
import com.intellij.psi.codeStyle.CommonCodeStyleSettings
import com.intellij.psi.impl.source.codeStyle.PostFormatProcessor
import com.intellij.psi.impl.source.codeStyle.PostFormatProcessorHelper
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile

/**
 * Wrapping and Braces | Force braces, as Java has it: Reformat Code gives a
 * single-statement body of `if`, `else`, `for`, `while` and `do` braces,
 * always or when the statement spans several lines, per the "'if()'
 * statement", "'for()' statement", "'while()' statement" and "'do ...
 * while()' statement" options. `else if` is never wrapped.
 *
 * It runs after the formatter, like Java's brace enforcer, and formats each
 * block it creates.
 */
class JuxBraceEnforcer : PostFormatProcessor {

    override fun processElement(source: PsiElement, settings: CodeStyleSettings): PsiElement {
        if (source.containingFile !is JuxFile) return source
        enforce(source.containingFile, source.textRange, settings)
        return source
    }

    override fun processText(source: PsiFile, rangeToReformat: TextRange, settings: CodeStyleSettings): TextRange {
        if (source !is JuxFile) return rangeToReformat
        val helper = PostFormatProcessorHelper(settings.getCommonSettings(JuxLanguage))
        helper.resultTextRange = rangeToReformat
        enforce(source, rangeToReformat, settings, helper)
        return helper.resultTextRange
    }

    private fun enforce(file: PsiFile, range: TextRange, settings: CodeStyleSettings, helper: PostFormatProcessorHelper? = null) {
        val common = settings.getCommonSettings(JuxLanguage)
        if (common.IF_BRACE_FORCE == CommonCodeStyleSettings.DO_NOT_FORCE &&
            common.FOR_BRACE_FORCE == CommonCodeStyleSettings.DO_NOT_FORCE &&
            common.WHILE_BRACE_FORCE == CommonCodeStyleSettings.DO_NOT_FORCE &&
            common.DOWHILE_BRACE_FORCE == CommonCodeStyleSettings.DO_NOT_FORCE
        ) return
        // Innermost last: wrapping an outer body first would detach the inner
        // statements this list holds.
        val statements = PsiTreeUtil.collectElements(file) { it.elementType in OWNERS && range.contains(it.textRange) }
            .sortedByDescending { it.textRange.startOffset }
        for (statement in statements) {
            if (!statement.isValid) continue
            val option = when (statement.elementType) {
                E.IF_STATEMENT -> common.IF_BRACE_FORCE
                E.FOR_STATEMENT, E.FOR_EACH_STATEMENT -> common.FOR_BRACE_FORCE
                E.WHILE_STATEMENT -> common.WHILE_BRACE_FORCE
                else -> common.DOWHILE_BRACE_FORCE
            }
            if (option == CommonCodeStyleSettings.DO_NOT_FORCE) continue
            if (option == CommonCodeStyleSettings.FORCE_BRACES_IF_MULTILINE && !statement.textContains('\n')) continue
            val bodies = bodies(statement)
            if (bodies.isEmpty()) continue
            val oldLength = statement.textLength
            for (body in bodies) body.replace(JuxElementFactory.createCodeBlock(statement.project, body.text))
            // The whole statement, not just the new blocks: `}\nelse` has to
            // cuddle now that a `}` is there to cuddle with.
            val formatted = CodeStyleManager.getInstance(statement.project).reformat(statement)
            helper?.updateResultRange(oldLength, formatted.textLength)
        }
    }

    /** The single-statement bodies of [statement] that are not blocks already. */
    private fun bodies(statement: PsiElement): List<PsiElement> {
        val children = statement.children.toList()
        val out = ArrayList<PsiElement>()
        when (statement.elementType) {
            E.IF_STATEMENT -> {
                // `if (c) then else other`: the then-branch follows the `)`,
                // the else-branch follows `else`.
                var sawParen = false
                var sawElse = false
                var c = statement.firstChild
                while (c != null) {
                    when {
                        c.elementType === T.RPAREN && !sawParen -> sawParen = true
                        c.elementType === T.ELSE_KW -> sawElse = true
                        c in children && sawParen && !sawElse && out.isEmpty() -> out.add(c)
                        c in children && sawElse -> {
                            if (c.elementType !== E.IF_STATEMENT) out.add(c)
                            break
                        }
                    }
                    c = c.nextSibling
                }
            }
            E.DO_WHILE_STATEMENT -> children.firstOrNull()?.let { out.add(it) }
            else -> children.lastOrNull()?.let { out.add(it) }
        }
        return out.filter { it.elementType in STATEMENTS }
    }

    private companion object {
        /** Statements whose body the options cover. */
        val OWNERS = setOf(E.IF_STATEMENT, E.FOR_STATEMENT, E.FOR_EACH_STATEMENT, E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT)

        /** A body that is one statement (not a block, not an empty `;`). */
        val STATEMENTS = setOf(
            E.EXPRESSION_STATEMENT, E.LOCAL_VARIABLE, E.IF_STATEMENT, E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT,
            E.FOR_STATEMENT, E.FOR_EACH_STATEMENT, E.SWITCH_STATEMENT, E.RETURN_STATEMENT, E.BREAK_STATEMENT,
            E.CONTINUE_STATEMENT, E.THROW_STATEMENT, E.TRY_STATEMENT, E.UNSAFE_STATEMENT, E.LABELED_STATEMENT,
        )
    }
}
