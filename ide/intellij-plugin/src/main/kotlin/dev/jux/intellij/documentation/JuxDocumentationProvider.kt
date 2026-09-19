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

    /**
     * The full Ctrl+Q popup, laid out as Java's: where the declaration lives
     * (its type, or its file for a top-level one), its signature, with the
     * inferred type of a `var`, then the doc comment's description and its
     * Params, Returns, Throws, Since and See sections. A method without a doc
     * comment of its own shows the one of the method it overrides, as Java's
     * "Description copied from" does. Generated `.jux.d` stubs are Jux files
     * too, so a library member's doc renders the same way.
     */
    override fun generateDoc(element: PsiElement?, originalElement: PsiElement?): String? {
        val decl = element as? JuxNamedElement ?: return null
        val sb = StringBuilder()
        sb.append(DocumentationMarkup.DEFINITION_START)
        location(decl)?.let { sb.append("<small>").append(StringUtil.escapeXmlEntities(it)).append("</small><br/>") }
        sb.append(StringUtil.escapeXmlEntities(signature(decl)))
        inferredType(decl)?.let { sb.append(": ").append(StringUtil.escapeXmlEntities(it)) }
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
        var copiedFrom: String? = null
        val doc = docComment(decl) ?: inheritedDoc(decl)?.let { (from, text) -> copiedFrom = from; text }
        if (doc != null) renderDoc(JuxDocComment.parse(doc), copiedFrom, sb)
        return sb.toString()
    }

    /** The description, then one section per tag kind, as Java's popup shows them. */
    private fun renderDoc(doc: JuxDocComment, copiedFrom: String?, sb: StringBuilder) {
        if (doc.description.isNotEmpty()) {
            sb.append(DocumentationMarkup.CONTENT_START)
            if (copiedFrom != null) {
                sb.append("<p><i>Description copied from: ").append(StringUtil.escapeXmlEntities(copiedFrom)).append("</i></p>")
            }
            sb.append(JuxDocComment.toHtml(doc.description))
            sb.append(DocumentationMarkup.CONTENT_END)
        }
        val sections = listOf(
            "Params:" to doc.params.map { (name, text) -> "<code>$name</code> - ${JuxDocComment.toHtml(text)}" },
            "Returns:" to listOfNotNull(doc.returns?.let { JuxDocComment.toHtml(it) }),
            "Throws:" to doc.throws.map { (type, text) -> "<code>$type</code> - ${JuxDocComment.toHtml(text)}" },
            "Since:" to listOfNotNull(doc.since?.let { JuxDocComment.toHtml(it) }),
            "See Also:" to doc.see.map { "<code>${StringUtil.escapeXmlEntities(it)}</code>" },
        ).filter { it.second.isNotEmpty() }
        if (sections.isEmpty()) return
        sb.append(DocumentationMarkup.SECTIONS_START)
        for ((title, rows) in sections) {
            sb.append(DocumentationMarkup.SECTION_HEADER_START).append(title).append(DocumentationMarkup.SECTION_SEPARATOR)
            sb.append(rows.joinToString("<br/>"))
            sb.append(DocumentationMarkup.SECTION_END)
        }
        sb.append(DocumentationMarkup.SECTIONS_END)
    }

    /** `Cart`, or `shop.Cart` for a member; the file for a top-level declaration. */
    private fun location(decl: JuxNamedElement): String? {
        if (decl.elementType === E.PARAMETER || decl.elementType === E.LOCAL_VARIABLE) return null
        val owner = com.intellij.psi.util.PsiTreeUtil.getParentOfType(decl, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)
        val pkg = decl.containingFile?.let { dev.jux.intellij.run.JuxTestDetector.packageName(it) }.orEmpty()
        return when {
            owner != null -> listOf(pkg, owner.name.orEmpty()).filter { it.isNotEmpty() }.joinToString(".")
            decl.containingFile != null -> listOf(pkg, decl.containingFile.name).filter { it.isNotEmpty() }.joinToString(" ")
            else -> null
        }.takeIf { !it.isNullOrEmpty() }
    }

    /** The type a `var` declaration infers, which its signature does not show. */
    private fun inferredType(decl: JuxNamedElement): String? {
        if (decl.elementType !== E.LOCAL_VARIABLE && decl.elementType !== E.FIELD_DECLARATION) return null
        if (decl.node.findChildByType(E.TYPE_REFERENCE) != null && decl.node.findChildByType(JuxTokenTypes.VAR_KW) == null) return null
        val type = dev.jux.intellij.resolve.JuxTypeEngine.declaredType(decl)
        return type.takeIf { it !is dev.jux.intellij.resolve.JuxType.Unknown }?.presentable()
    }

    /** The doc of the nearest method [decl] overrides that has one, with where it came from. */
    private fun inheritedDoc(decl: JuxNamedElement): Pair<String, String>? {
        val method = decl as? dev.jux.intellij.psi.JuxMethodDeclaration ?: return null
        for (sup in dev.jux.intellij.codeInsight.JuxGotoSuperHandler.superMethods(method)) {
            val named = sup as? JuxNamedElement ?: continue
            val text = docComment(named) ?: continue
            val owner = com.intellij.psi.util.PsiTreeUtil.getParentOfType(sup, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)?.name
            return "${owner ?: "?"}.${named.name}" to text
        }
        return null
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
