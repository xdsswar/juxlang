package dev.jux.intellij.highlight

import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.Annotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType

/**
 * String-interior highlighting — the layer that lights up what the lexer
 * deliberately keeps as one token:
 *
 * - **escape sequences** in `"…"`, `'…'`, and `$"…"` literals: the valid set
 *   mirrors the compiler's `process_string_escapes` (`crates/juxc-parse/src/
 *   literals.rs`) — `\n \r \t \b \f \0 \\ \' \" \$`, `\xHH`, `\u{H+}`. Valid
 *   escapes get [JuxSyntaxHighlighter.VALID_ESCAPE]; malformed ones get
 *   [JuxSyntaxHighlighter.INVALID_ESCAPE] (colour only — the compiler owns the
 *   actual diagnostic). Raw strings (`"""…"""`) are verbatim: no escapes.
 * - **interpolation holes** in `$"…${expr}…"` / `$"""…"""`: the `${` and `}`
 *   delimiters get [JuxSyntaxHighlighter.INTERPOLATION], and the expression
 *   between them is re-lexed with [JuxLexer] so keywords / numbers / nested
 *   strings inside the hole are coloured exactly like top-level code — with
 *   zero parser involvement.
 */
class JuxStringAnnotator : Annotator {
    override fun annotate(element: PsiElement, holder: AnnotationHolder) {
        when (element.elementType) {
            JuxTokenTypes.STRING_LITERAL, JuxTokenTypes.CHAR_LITERAL ->
                annotateEscapes(element, holder, interp = false)
            JuxTokenTypes.INTERP_STRING_LITERAL -> {
                annotateEscapes(element, holder, interp = true)
                annotateInterpolations(element, holder)
            }
            JuxTokenTypes.INTERP_RAW_STRING_LITERAL ->
                // Raw: no escapes, but `${…}` holes still interpolate.
                annotateInterpolations(element, holder)
        }
    }

    // ---- escapes -----------------------------------------------------------

    /**
     * Walks the literal's text for `\…` sequences and colours each. When
     * [interp] is set, escapes inside `${…}` holes are skipped (that text is
     * expression code, handled by [annotateInterpolations]).
     */
    private fun annotateEscapes(element: PsiElement, holder: AnnotationHolder, interp: Boolean) {
        val text = element.text
        val base = element.textRange.startOffset
        var i = 0
        var braceDepth = 0
        while (i < text.length) {
            val c = text[i]
            if (interp && braceDepth == 0 && c == '$' && i + 1 < text.length && text[i + 1] == '{') {
                braceDepth = 1; i += 2; continue
            }
            if (braceDepth > 0) {
                when (c) { '{' -> braceDepth++; '}' -> braceDepth-- }
                i++; continue
            }
            if (c != '\\' || i + 1 >= text.length) { i++; continue }

            val (len, valid) = classifyEscape(text, i)
            val range = TextRange(base + i, base + i + len)
            mark(holder, range, if (valid) JuxSyntaxHighlighter.VALID_ESCAPE else JuxSyntaxHighlighter.INVALID_ESCAPE)
            i += len
        }
    }

    /**
     * Classifies the escape starting at `text[start] == '\\'`: returns its
     * total length and whether it is one of the compiler-valid forms.
     */
    private fun classifyEscape(text: String, start: Int): Pair<Int, Boolean> {
        val c = text.getOrNull(start + 1) ?: return 1 to false
        return when (c) {
            'n', 'r', 't', 'b', 'f', '0', '\\', '\'', '"', '$' -> 2 to true
            'x' -> {
                // `\xHH` — exactly two hex digits, ≤ 0x7F.
                val hex = text.substring(start + 2, minOf(start + 4, text.length))
                val ok = hex.length == 2 && hex.all { it.isHexDigit() } &&
                    hex.toInt(16) <= 0x7F
                (2 + hex.length) to ok
            }
            'u' -> {
                // `\u{H+}` — up to six hex digits in braces.
                if (text.getOrNull(start + 2) != '{') return 2 to false
                val close = text.indexOf('}', start + 3)
                if (close < 0) return 2 to false
                val hex = text.substring(start + 3, close)
                val ok = hex.isNotEmpty() && hex.length <= 6 && hex.all { it.isHexDigit() }
                (close - start + 1) to ok
            }
            else -> 2 to false
        }
    }

    private fun Char.isHexDigit() = this in '0'..'9' || this in 'a'..'f' || this in 'A'..'F'

    // ---- interpolation holes ----------------------------------------------

    /**
     * Finds each `${…}` hole (brace-balanced, matching the lexer's scan),
     * colours its delimiters, and re-lexes the interior so the embedded
     * expression highlights like real code.
     */
    private fun annotateInterpolations(element: PsiElement, holder: AnnotationHolder) {
        val text = element.text
        val base = element.textRange.startOffset
        var i = 0
        while (i < text.length - 1) {
            if (text[i] == '\\') { i += 2; continue }
            if (text[i] != '$' || text[i + 1] != '{') { i++; continue }

            val exprStart = i + 2
            var depth = 1
            var j = exprStart
            while (j < text.length && depth > 0) {
                when (text[j]) { '{' -> depth++; '}' -> depth-- }
                j++
            }
            val exprEnd = if (depth == 0) j - 1 else j // exclusive of `}` when closed

            mark(holder, TextRange(base + i, base + exprStart), JuxSyntaxHighlighter.INTERPOLATION)
            highlightFragment(text.substring(exprStart, exprEnd), base + exprStart, holder)
            if (depth == 0) {
                mark(holder, TextRange(base + exprEnd, base + exprEnd + 1), JuxSyntaxHighlighter.INTERPOLATION)
            }
            i = j
        }
    }

    /** Re-lexes [fragment] and applies the lexer-level colour map per token. */
    private fun highlightFragment(fragment: String, baseOffset: Int, holder: AnnotationHolder) {
        if (fragment.isBlank()) return
        val lexer = JuxLexer()
        lexer.start(fragment, 0, fragment.length, 0)
        while (true) {
            val type = lexer.tokenType ?: break
            val keys = HIGHLIGHTER.getTokenHighlights(type)
            if (keys.isNotEmpty()) {
                mark(
                    holder,
                    TextRange(baseOffset + lexer.tokenStart, baseOffset + lexer.tokenEnd),
                    keys[0],
                )
            }
            lexer.advance()
        }
    }

    private fun mark(holder: AnnotationHolder, range: TextRange, key: TextAttributesKey) {
        if (range.isEmpty) return
        holder.newSilentAnnotation(HighlightSeverity.INFORMATION)
            .range(range)
            .textAttributes(key)
            .create()
    }

    private companion object {
        /** Shared, stateless — only used for its token→key map. */
        val HIGHLIGHTER = JuxSyntaxHighlighter()
    }
}
