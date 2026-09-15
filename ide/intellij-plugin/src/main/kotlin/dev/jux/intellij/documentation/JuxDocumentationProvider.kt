package dev.jux.intellij.documentation

import com.intellij.lang.documentation.AbstractDocumentationProvider
import com.intellij.lang.documentation.DocumentationMarkup
import com.intellij.openapi.util.text.StringUtil
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxPropertyDeclaration

/**
 * Quick Documentation (Ctrl+Q) and the navigation-bar tooltip for Jux
 * declarations. Renders the declaration's **signature** (everything up to its
 * body / `;`) plus the leading `/** … */` or `///` doc comment, IntelliJ-style.
 *
 * Works off the native PSI, so it functions on Community IDEs without the LSP;
 * cross-file targets resolve through [dev.jux.intellij.resolve.JuxReference]
 * before reaching this provider.
 */
class JuxDocumentationProvider : AbstractDocumentationProvider(), com.intellij.lang.documentation.CodeDocumentationProvider {

    // ---- `/**` + Enter: the documentation stub ---------------------------------

    override fun findExistingDocComment(contextElement: com.intellij.psi.PsiComment?): com.intellij.psi.PsiComment? =
        contextElement

    /** The comment and the declaration it documents: the next non-space sibling. */
    override fun parseContext(startPoint: PsiElement): com.intellij.openapi.util.Pair<PsiElement, com.intellij.psi.PsiComment>? {
        var comment: PsiElement? = startPoint
        while (comment != null && comment !is com.intellij.psi.PsiComment) comment = comment.parent
        val doc = comment as? com.intellij.psi.PsiComment ?: return null
        val owner = documentedDeclaration(doc) ?: return null
        return com.intellij.openapi.util.Pair.create(owner, doc)
    }

    /**
     * The tags Java writes for a method: one `@param` per parameter (type
     * parameters as `@param <T>`), `@return` unless it returns `void`, and one
     * `@throws` per declared exception. Constructors get parameters only.
     */
    override fun generateDocumentationContentStub(contextComment: com.intellij.psi.PsiComment?): String? {
        val comment = contextComment ?: return null
        val decl = documentedDeclaration(comment) ?: return null
        val type = decl.node.elementType
        if (type !== E.METHOD_DECLARATION && type !== E.CONSTRUCTOR_DECLARATION && type !== E.OPERATOR_DECLARATION) {
            return null
        }
        val sb = StringBuilder()
        decl.node.findChildByType(E.TYPE_PARAMETER_LIST)?.psi?.children
            ?.filterIsInstance<JuxNamedElement>()
            ?.forEach { tp -> tp.name?.let { sb.append("* @param <").append(it).append(">\n") } }
        decl.node.findChildByType(E.PARAMETER_LIST)?.psi?.children
            ?.filter { it.elementType === E.PARAMETER }
            ?.forEach { p -> (p as? JuxNamedElement)?.name?.let { sb.append("* @param ").append(it).append("\n") } }
        if (type === E.METHOD_DECLARATION || type === E.OPERATOR_DECLARATION) {
            val returnType = decl.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()
            if (returnType != null && returnType != "void") sb.append("* @return\n")
        }
        decl.node.findChildByType(E.THROWS_CLAUSE)?.psi?.children
            ?.filter { it.elementType === E.TYPE_REFERENCE }
            ?.forEach { sb.append("* @throws ").append(it.text.trim()).append("\n") }
        return sb.toString()
    }

    private fun documentedDeclaration(comment: PsiElement): PsiElement? {
        var next = comment.nextSibling
        while (next != null && (next is com.intellij.psi.PsiWhiteSpace || next is com.intellij.psi.PsiComment)) {
            next = next.nextSibling
        }
        // A doc comment on the first member of a body sits before the member node.
        return next?.takeIf { it is JuxNamedElement }
    }

    /** The one-line summary shown in the navigation bar / Ctrl+hover preview. */
    override fun getQuickNavigateInfo(element: PsiElement?, originalElement: PsiElement?): String? {
        val decl = element as? JuxNamedElement ?: return null
        return "${kindLabel(decl)} ${signature(decl)}${nullableSuffix(decl)}"
    }

    /** The full Ctrl+Q popup: a definition block plus the doc comment, if any. */
    override fun generateDoc(element: PsiElement?, originalElement: PsiElement?): String? {
        val decl = element as? JuxNamedElement ?: return null
        val sb = StringBuilder()
        sb.append(DocumentationMarkup.DEFINITION_START)
        sb.append(StringUtil.escapeXmlEntities(signature(decl)))
        sb.append(DocumentationMarkup.DEFINITION_END)
        // An uninitialized auto-property is implicitly nullable (§M.7.3.1): note
        // the effective `T?` type the program actually sees, so offline tooling
        // is honest about nullability without an LSP session.
        implicitNullableType(decl)?.let { eff ->
            sb.append(DocumentationMarkup.CONTENT_START)
            sb.append("Implicitly nullable: reads <code>")
            sb.append(StringUtil.escapeXmlEntities(eff))
            sb.append("</code> (null until set).")
            sb.append(DocumentationMarkup.CONTENT_END)
        }
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
     * The effective `T?` type when `decl` is an implicitly-nullable auto-property
     * (uninitialized, §M.7.3.1), else null. Drives the hover nullability note.
     */
    private fun implicitNullableType(decl: JuxNamedElement): String? =
        (decl as? JuxPropertyDeclaration)?.takeIf { it.isImplicitlyNullable() }?.effectiveTypeText()

    /** ` : T?` appended to the nav-bar summary of an implicitly-nullable property. */
    private fun nullableSuffix(decl: JuxNamedElement): String =
        implicitNullableType(decl)?.let { " : $it" } ?: ""

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
