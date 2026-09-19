package dev.jux.intellij.documentation

import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.Annotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxSyntaxHighlighter
import dev.jux.intellij.highlight.JuxTokenTypes

/**
 * Colors the inside of a `/** ... */` doc comment the way `jux doc` reads it:
 *
 *  - the block tags (`@param`, `@return`, `@throws`, `@deprecated`, `@since`,
 *    `@see`) as doc tags, and the name after `@param` / `@throws` as a tag
 *    value, as Java's editor does;
 *  - a ```` ```jux ```` example block as real Jux code, token by token with the
 *    editor's own colors, since `jux test --doc` compiles it;
 *  - the block's `ignore` / `no_run` flags as metadata, so what the doc
 *    examples will do with the block is visible where it is written.
 *
 * Each code line is lexed on its own, after the ` * ` margin, so the margin
 * never reaches the lexer. Silent annotations: color only, no messages.
 */
class JuxDocCommentAnnotator : Annotator {

    private val highlighter = JuxSyntaxHighlighter()

    override fun annotate(element: PsiElement, holder: AnnotationHolder) {
        if (element.elementType !== JuxTokenTypes.DOC_COMMENT) return
        val text = element.text
        val base = element.textRange.startOffset

        for (tag in JuxDocFences.tags(text)) {
            color(holder, base + tag.start, base + tag.end, DefaultLanguageHighlighterColors.DOC_COMMENT_TAG)
            tag.value?.let { color(holder, base + it.start, base + it.end, DefaultLanguageHighlighterColors.DOC_COMMENT_TAG_VALUE) }
        }

        for (fence in JuxDocFences.fences(text)) {
            for ((i, word) in fence.info.withIndex()) {
                val key = when {
                    i == 0 -> DefaultLanguageHighlighterColors.KEYWORD
                    word.text in JuxDocFences.FLAGS -> DefaultLanguageHighlighterColors.METADATA
                    else -> continue
                }
                color(holder, base + word.start, base + word.end, key)
            }
            if (!fence.isJux) continue
            for (line in fence.lines) highlightCode(holder, text, base, line)
        }
    }

    /** Lex one code line and color each token with the editor's Jux colors. */
    private fun highlightCode(holder: AnnotationHolder, text: String, base: Int, line: IntRange) {
        if (line.isEmpty()) return
        val lexer = highlighter.highlightingLexer
        lexer.start(text, line.first, line.last + 1)
        while (lexer.tokenType != null) {
            val keys = highlighter.getTokenHighlights(lexer.tokenType!!)
            keys.lastOrNull()?.let { color(holder, base + lexer.tokenStart, base + lexer.tokenEnd, it) }
            lexer.advance()
        }
    }

    private fun color(holder: AnnotationHolder, start: Int, end: Int, key: com.intellij.openapi.editor.colors.TextAttributesKey) {
        if (end <= start) return
        holder.newSilentAnnotation(HighlightSeverity.INFORMATION)
            .range(TextRange(start, end))
            .textAttributes(key)
            .create()
    }
}
