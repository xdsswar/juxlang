package dev.jux.intellij.highlight

import com.intellij.lexer.Lexer
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors as D
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.editor.colors.TextAttributesKey.createTextAttributesKey
import com.intellij.openapi.editor.markup.TextAttributes
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase
import com.intellij.psi.tree.IElementType
import com.intellij.ui.JBColor
import java.awt.Font

/**
 * Maps the [JuxLexer]'s fine-grained tokens to colour attribute keys. Each key
 * inherits from a `DefaultLanguageHighlighterColors` base, so Jux follows the
 * active theme and stays customizable in **Settings → Editor → Color Scheme →
 * Jux** (see [JuxColorSettingsPage]).
 *
 * This is the lexer-level (syntactic) layer. Role-sensitive colouring —
 * primitive type names, annotations, declarations vs references — is added by
 * the PSI-based semantic highlighter once the parser lands.
 */
class JuxSyntaxHighlighter : SyntaxHighlighterBase() {
    override fun getHighlightingLexer(): Lexer = JuxLexer()

    override fun getTokenHighlights(tokenType: IElementType): Array<TextAttributesKey> =
        SyntaxHighlighterBase.pack(KEYS[tokenType])

    companion object {
        val KEYWORD = key("JUX_KEYWORD", D.KEYWORD)
        // Set by the PSI semantic highlighter ([dev.jux.intellij.highlight.JuxAnnotator]):
        // primitive type names are identifiers at the lexer level, so they (and
        // declaration names) are coloured from the parse tree, not the lexer.
        val TYPE = key("JUX_TYPE", D.KEYWORD)
        val CLASS_NAME = key("JUX_CLASS_NAME", D.CLASS_NAME)
        val METHOD_DECLARATION = key("JUX_METHOD_DECLARATION", D.FUNCTION_DECLARATION)
        val FIELD = key("JUX_FIELD", D.INSTANCE_FIELD)
        // Reference-side colours (decl-vs-use), also annotator-driven: a call
        // site, a parameter/local read, a type-parameter use, an enum constant.
        val METHOD_CALL = key("JUX_METHOD_CALL", D.FUNCTION_CALL)
        val PARAMETER = key("JUX_PARAMETER", D.PARAMETER)
        val LOCAL_VARIABLE = key("JUX_LOCAL_VARIABLE", D.LOCAL_VARIABLE)
        // Generic type parameters (`T`, `R`, …) — a DARKER GREEN hardcoded as
        // the key's default so it shows on EVERY color scheme (the per-scheme
        // bundled defaults only cover named schemes; custom themes fell back to
        // the plain parameter color). JBColor adapts: a deep green on light
        // backgrounds, a darker-but-readable green on dark ones. Users can
        // still recolor it under Color Scheme | Jux | References | Type parameter.
        val TYPE_PARAMETER = keyWithDefault(
            "JUX_TYPE_PARAMETER",
            TextAttributes(JBColor(0x1E6B1E, 0x4C8A4C), null, null, null, Font.PLAIN),
        )
        val ENUM_CONSTANT = key("JUX_ENUM_CONSTANT", D.STATIC_FIELD)
        // §P.5 native coloring (annotator-driven, property context only): the
        // attach/detach/clear/size + bind/unbind/bindBidirectional operations.
        // KEYWORD-based, signalling "built-in, not user code" on every theme.
        // (The `.observers` member itself is colored as a primitive/TYPE — see
        // JuxAnnotator.propertyContextColor — so it matches the `observer` type.)
        val NATIVE_OPERATION = key("JUX_NATIVE_OPERATION", D.KEYWORD)
        // String-interior colours ([JuxStringAnnotator]): `${…}` delimiters
        // and escape sequences (valid vs malformed).
        val INTERPOLATION = key("JUX_INTERPOLATION", D.KEYWORD)
        // Variable reads inside an interpolation hole (`${name}`) and the
        // `$name` shorthand. A muted PURPLE ITALIC is hardcoded as the default
        // (the same "show on every scheme" trick as TYPE_PARAMETER) so the
        // interpolated names pop the way Java instance fields do — even on
        // custom color schemes that don't bundle a Jux mapping. JBColor adapts:
        // a deep mauve on light backgrounds, the familiar Darcula field purple
        // on dark ones. Recolor under Color Scheme | Jux | String | Interpolated
        // variable. (Call NAMES inside a hole stay code-colored — see
        // JuxStringAnnotator.highlightFragment.)
        val INTERPOLATED_VARIABLE = keyWithDefault(
            "JUX_INTERPOLATED_VARIABLE",
            TextAttributes(JBColor(0x876885, 0x9876AA), null, null, null, Font.ITALIC),
        )
        val VALID_ESCAPE = key("JUX_VALID_ESCAPE", D.VALID_STRING_ESCAPE)
        val INVALID_ESCAPE = key("JUX_INVALID_ESCAPE", D.INVALID_STRING_ESCAPE)
        val CONSTANT = key("JUX_CONSTANT", D.KEYWORD)
        val STRING = key("JUX_STRING", D.STRING)
        val CHAR = key("JUX_CHAR", D.STRING)
        val NUMBER = key("JUX_NUMBER", D.NUMBER)
        val LINE_COMMENT = key("JUX_LINE_COMMENT", D.LINE_COMMENT)
        val BLOCK_COMMENT = key("JUX_BLOCK_COMMENT", D.BLOCK_COMMENT)
        val DOC_COMMENT = key("JUX_DOC_COMMENT", D.DOC_COMMENT)
        val ANNOTATION = key("JUX_ANNOTATION", D.METADATA)
        val OPERATOR = key("JUX_OPERATOR", D.OPERATION_SIGN)
        val BRACES = key("JUX_BRACES", D.BRACES)
        val BRACKETS = key("JUX_BRACKETS", D.BRACKETS)
        val PARENS = key("JUX_PARENS", D.PARENTHESES)
        val SEMICOLON = key("JUX_SEMICOLON", D.SEMICOLON)
        val COMMA = key("JUX_COMMA", D.COMMA)
        val DOT = key("JUX_DOT", D.DOT)
        val IDENTIFIER = key("JUX_IDENTIFIER", D.IDENTIFIER)

        private fun key(externalName: String, base: TextAttributesKey) =
            createTextAttributesKey(externalName, base)

        /**
         * Registers a key whose default attributes are baked in as a literal
         * [TextAttributes] rather than inherited from a base key.
         *
         * The platform deprecates the `String` + `TextAttributes` overload
         * because hardcoded attributes bypass color schemes, but we use it
         * deliberately for [TYPE_PARAMETER] and [INTERPOLATED_VARIABLE]: a
         * [JBColor] default makes the color appear on EVERY scheme, including
         * custom themes that bundle no Jux mapping (see those fields' comments).
         * The supported `additionalTextAttributes` XML route only covers the
         * named schemes we ship, which is exactly the gap this closes. Users can
         * still recolor both keys under Settings | Editor | Color Scheme | Jux.
         */
        @Suppress("DEPRECATION")
        private fun keyWithDefault(externalName: String, default: TextAttributes) =
            createTextAttributesKey(externalName, default)

        private val KEYS: Map<IElementType, TextAttributesKey> = buildMap {
            fun fill(set: com.intellij.psi.tree.TokenSet, value: TextAttributesKey) {
                for (t in set.types) put(t, value)
            }
            fill(JuxTokenTypes.KEYWORDS, KEYWORD)
            fill(JuxTokenTypes.OPERATORS, OPERATOR)
            fill(JuxTokenTypes.STRING_LITERALS, STRING)
            // Char literals share the string colour family but keep a distinct key.
            put(JuxTokenTypes.CHAR_LITERAL, CHAR)
            put(JuxTokenTypes.INT_LITERAL, NUMBER)
            put(JuxTokenTypes.FLOAT_LITERAL, NUMBER)
            put(JuxTokenTypes.BOOL_LITERAL, CONSTANT)
            put(JuxTokenTypes.NULL_LITERAL, CONSTANT)
            put(JuxTokenTypes.LINE_COMMENT, LINE_COMMENT)
            put(JuxTokenTypes.BLOCK_COMMENT, BLOCK_COMMENT)
            put(JuxTokenTypes.DOC_COMMENT, DOC_COMMENT)
            put(JuxTokenTypes.AT, ANNOTATION)
            fill(JuxTokenTypes.BRACES, BRACES)
            fill(JuxTokenTypes.BRACKETS, BRACKETS)
            fill(JuxTokenTypes.PARENS, PARENS)
            put(JuxTokenTypes.SEMICOLON, SEMICOLON)
            put(JuxTokenTypes.COMMA, COMMA)
            put(JuxTokenTypes.DOT, DOT)
            put(JuxTokenTypes.IDENTIFIER, IDENTIFIER)
        }
    }
}
