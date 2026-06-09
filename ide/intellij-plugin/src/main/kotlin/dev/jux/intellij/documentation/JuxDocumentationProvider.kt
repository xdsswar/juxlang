package dev.jux.intellij.documentation

import com.intellij.lang.documentation.AbstractDocumentationProvider
import com.intellij.lang.documentation.DocumentationMarkup
import com.intellij.openapi.util.text.StringUtil
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/**
 * Quick Documentation (Ctrl+Q) and the navigation-bar tooltip for Jux
 * declarations. Renders the declaration's **signature** (everything up to its
 * body / `;`) plus the leading `/** … */` or `///` doc comment, IntelliJ-style.
 *
 * Works off the native PSI, so it functions on Community IDEs without the LSP;
 * cross-file targets resolve through [dev.jux.intellij.resolve.JuxReference]
 * before reaching this provider.
 */
class JuxDocumentationProvider : AbstractDocumentationProvider() {
    /** The one-line summary shown in the navigation bar / Ctrl+hover preview. */
    override fun getQuickNavigateInfo(element: PsiElement?, originalElement: PsiElement?): String? {
        val decl = element as? JuxNamedElement ?: return null
        return "${kindLabel(decl)} ${signature(decl)}"
    }

    /** The full Ctrl+Q popup: a definition block plus the doc comment, if any. */
    override fun generateDoc(element: PsiElement?, originalElement: PsiElement?): String? {
        val decl = element as? JuxNamedElement ?: return null
        val sb = StringBuilder()
        sb.append(DocumentationMarkup.DEFINITION_START)
        sb.append(StringUtil.escapeXmlEntities(signature(decl)))
        sb.append(DocumentationMarkup.DEFINITION_END)
        docComment(decl)?.let { doc ->
            sb.append(DocumentationMarkup.CONTENT_START)
            sb.append(StringUtil.escapeXmlEntities(doc).replace("\n", "<br/>"))
            sb.append(DocumentationMarkup.CONTENT_END)
        }
        return sb.toString()
    }

    /** A short human label for the declaration kind. */
    private fun kindLabel(decl: JuxNamedElement): String = when (decl.elementType) {
        E.CLASS_DECLARATION, E.STRUCT_DECLARATION -> "class"
        E.INTERFACE_DECLARATION -> "interface"
        E.ENUM_DECLARATION -> "enum"
        E.RECORD_DECLARATION -> "record"
        E.ANNOTATION_DECLARATION -> "annotation"
        E.TYPE_ALIAS_DECLARATION -> "type"
        E.METHOD_DECLARATION, E.OPERATOR_DECLARATION -> "method"
        E.CONSTRUCTOR_DECLARATION -> "constructor"
        E.FIELD_DECLARATION, E.PROPERTY_DECLARATION, E.CONST_DECLARATION -> "field"
        E.ENUM_CONSTANT -> "enum constant"
        E.PARAMETER -> "parameter"
        E.LOCAL_VARIABLE -> "local"
        else -> "declaration"
    }

    /**
     * The declaration's header — its text up to the body `{` or trailing `;`,
     * whitespace-collapsed. `public int area() { … }` → `public int area()`;
     * `public class Foo extends Bar { … }` → `public class Foo extends Bar`.
     */
    private fun signature(decl: JuxNamedElement): String {
        val text = decl.text
        val end = text.indexOfFirst { it == '{' || it == ';' }.let { if (it < 0) text.length else it }
        return text.substring(0, end).trim().replace(WHITESPACE, " ")
    }

    /**
     * The doc comment immediately preceding `decl`: a `/** … */` block (stripped
     * of its markers) or a run of `///` lines. Whitespace between the comment
     * and the declaration is skipped. `null` when there's no leading comment.
     */
    private fun docComment(decl: JuxNamedElement): String? {
        var sib = decl.prevSibling
        while (sib != null && sib.text.isBlank()) sib = sib.prevSibling
        if (sib == null) return null
        return when (sib.elementType) {
            JuxTokenTypes.DOC_COMMENT, JuxTokenTypes.BLOCK_COMMENT -> cleanBlock(sib.text)
            JuxTokenTypes.LINE_COMMENT -> {
                // Gather a contiguous run of line comments, top to bottom.
                val lines = ArrayDeque<String>()
                var cur: PsiElement? = sib
                while (cur != null && cur.elementType === JuxTokenTypes.LINE_COMMENT) {
                    lines.addFirst(cleanLine(cur.text))
                    var prev = cur.prevSibling
                    while (prev != null && prev.text.isBlank() && !prev.text.contains("\n\n")) {
                        prev = prev.prevSibling
                    }
                    cur = prev
                }
                lines.joinToString("\n").trim().ifEmpty { null }
            }
            else -> null
        }
    }

    /** Strip `/** … */` (or `/* … */`) markers and leading `*` from each line. */
    private fun cleanBlock(text: String): String =
        text.removePrefix("/**").removePrefix("/*").removeSuffix("*/")
            .lines()
            .joinToString("\n") { it.trim().removePrefix("*").trim() }
            .trim()

    /** Strip the leading `///` or `//` from a line comment. */
    private fun cleanLine(text: String): String =
        text.removePrefix("///").removePrefix("//").trim()

    private companion object {
        val WHITESPACE = Regex("\\s+")
    }
}
