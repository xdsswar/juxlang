package dev.jux.intellij.folding

import com.intellij.codeInsight.folding.CodeFoldingSettings
import com.intellij.lang.ASTNode
import com.intellij.lang.folding.FoldingBuilderEx
import com.intellij.lang.folding.FoldingDescriptor
import com.intellij.openapi.editor.Document
import com.intellij.openapi.project.DumbAware
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.TokenType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * Code folding with Java's regions and defaults, from the PSI tree:
 *
 *  - the import list (`import ...`), collapsed by default;
 *  - the file header comment, collapsed by default;
 *  - documentation comments (`/** First sentence... */`);
 *  - multi-line block comments and runs of `//` lines;
 *  - type bodies, method bodies, lambda bodies, accessor lists, switch
 *    bodies, `native` blocks;
 *  - long string literals and multi-line raw strings.
 *
 * Whether a region starts collapsed follows the shared settings (imports,
 * file header, methods, documentation comments) and [JuxCodeFoldingSettings]
 * for the rest, so Settings | Editor | General | Code Folding drives Jux the
 * way it drives Java. Each descriptor carries its own placeholder and
 * collapse state, because one node kind (a comment, a code block) folds
 * differently depending on where it sits.
 */
class JuxFoldingBuilder : FoldingBuilderEx(), DumbAware {

    override fun buildFoldRegions(root: PsiElement, document: Document, quick: Boolean): Array<FoldingDescriptor> {
        val out = ArrayList<FoldingDescriptor>()
        foldImports(root.node, document, out)
        collect(root.node, document, out)
        return out.toTypedArray()
    }

    private fun collect(node: ASTNode, document: Document, out: MutableList<FoldingDescriptor>) {
        val type = node.elementType
        when {
            type === T.DOC_COMMENT && isMultiline(node.textRange, document) ->
                out.add(descriptor(node, node.textRange, docPlaceholder(node), docCollapsed(node)))
            type === T.BLOCK_COMMENT && isMultiline(node.textRange, document) ->
                out.add(descriptor(node, node.textRange, "/*...*/", commentCollapsed(node, JuxCodeFoldingSettings.getInstance().collapseMultilineComments)))
            type === T.LINE_COMMENT -> foldCommentRun(node, document, out)
            type === E.CODE_BLOCK && isMultiline(node.textRange, document) ->
                out.add(descriptor(node, node.textRange, "{...}", blockCollapsed(node)))
            type === E.CLASS_BODY && isMultiline(node.textRange, document) ->
                out.add(descriptor(node, node.textRange, "{...}", isInnerClassBody(node) && JuxCodeFoldingSettings.getInstance().collapseInnerClasses))
            type === E.PROPERTY_ACCESSOR_LIST && isMultiline(node.textRange, document) ->
                out.add(descriptor(node, node.textRange, "{...}", JuxCodeFoldingSettings.getInstance().collapseAccessors))
            type === E.EXTERN_BLOCK && isMultiline(node.textRange, document) ->
                out.add(descriptor(node, node.textRange, "{...}", false))
            // A switch's case list isn't a CODE_BLOCK, so fold it explicitly,
            // from its `{` (keeping `switch (expr)` visible) to the node end.
            type in SWITCHES -> braceRange(node)?.let { range ->
                if (isMultiline(range, document)) out.add(descriptor(node, range, "{...}", false))
            }
            type in RAW_STRINGS && isMultiline(node.textRange, document) ->
                out.add(descriptor(node, node.textRange, "\"\"\"...\"\"\"", JuxCodeFoldingSettings.getInstance().collapseLongStrings))
            type in STRINGS && node.textLength > LONG_STRING -> foldLongString(node, out)
        }
        var child = node.firstChildNode
        while (child != null) {
            collect(child, document, out)
            child = child.treeNext
        }
    }

    private fun descriptor(node: ASTNode, range: TextRange, placeholder: String, collapsed: Boolean) =
        FoldingDescriptor(node, range, null, placeholder, collapsed, emptySet())

    // ---- imports -----------------------------------------------------------

    /**
     * `import a.B; import c.D;` folds to `import ...`, as Java's list does:
     * from the first path to the end of the last import. A lone import is
     * left alone, also as Java does.
     */
    private fun foldImports(file: ASTNode, document: Document, out: MutableList<FoldingDescriptor>) {
        val imports = file.getChildren(null).filter { it.elementType === E.IMPORT_STATEMENT }
        if (imports.size < 2) return
        val first = imports.first()
        val keyword = first.findChildByType(T.IMPORT_KW) ?: return
        var start = keyword.treeNext
        while (start != null && start.elementType === TokenType.WHITE_SPACE) start = start.treeNext
        val range = TextRange(start?.startOffset ?: return, imports.last().textRange.endOffset)
        if (range.isEmpty) return
        out.add(descriptor(first, range, "...", CodeFoldingSettings.getInstance().COLLAPSE_IMPORTS))
    }

    // ---- comments ----------------------------------------------------------

    /**
     * A run of two or more `//` lines folds as one region, from the first
     * one that starts it; a line inside a run is skipped (the run's first
     * line already covers it).
     */
    private fun foldCommentRun(node: ASTNode, document: Document, out: MutableList<FoldingDescriptor>) {
        if (adjacentComment(node, forward = false) != null) return
        var last = node
        var count = 1
        while (true) {
            last = adjacentComment(last, forward = true) ?: break
            count++
        }
        if (count < 2) return
        val range = TextRange(node.startOffset, last.textRange.endOffset)
        if (!isMultiline(range, document)) return
        out.add(descriptor(node, range, "//...", commentCollapsed(node, JuxCodeFoldingSettings.getInstance().collapseEndOfLineComments)))
    }

    /** The `//` comment on the line next to [node] (before or after), or null. */
    private fun adjacentComment(node: ASTNode, forward: Boolean): ASTNode? {
        val space = (if (forward) node.treeNext else node.treePrev) ?: return null
        if (space.elementType !== TokenType.WHITE_SPACE || space.text.count { it == '\n' } != 1) return null
        val next = (if (forward) space.treeNext else space.treePrev) ?: return null
        return next.takeIf { it.elementType === T.LINE_COMMENT }
    }

    /**
     * A comment that opens the file, before its `package`, imports or first
     * declaration, is the file header: collapsed by the shared setting.
     */
    private fun commentCollapsed(node: ASTNode, otherwise: Boolean): Boolean =
        if (isFileHeader(node)) CodeFoldingSettings.getInstance().COLLAPSE_FILE_HEADER else otherwise

    private fun isFileHeader(node: ASTNode): Boolean {
        if (node.treeParent?.treeParent != null) return false // not at file level
        var prev = node.treePrev
        while (prev != null) {
            if (prev.elementType !== TokenType.WHITE_SPACE && prev.elementType !in COMMENTS) return false
            prev = prev.treePrev
        }
        // A file that is nothing but a comment has no header to hide.
        var next = node.treeNext
        while (next != null) {
            if (next.elementType !== TokenType.WHITE_SPACE && next.elementType !in COMMENTS) return true
            next = next.treeNext
        }
        return false
    }

    private fun docCollapsed(node: ASTNode): Boolean =
        if (isFileHeader(node)) CodeFoldingSettings.getInstance().COLLAPSE_FILE_HEADER
        else CodeFoldingSettings.getInstance().COLLAPSE_DOC_COMMENTS

    /** `/** Returns the total... */`: the doc comment's first sentence, as Java shows it. */
    private fun docPlaceholder(node: ASTNode): String {
        val text = node.text.removePrefix("/**").removeSuffix("*/")
            .lines()
            .map { it.trim().removePrefix("*").trim() }
            .filter { it.isNotEmpty() && !it.startsWith("@") }
            .joinToString(" ")
        if (text.isEmpty()) return "/**...*/"
        val sentence = text.substringBefore(". ").removeSuffix(".").trim()
        val shown = if (sentence.length > DOC_SUMMARY) sentence.take(DOC_SUMMARY).trimEnd() + "..." else sentence
        return "/** $shown */"
    }

    // ---- blocks ------------------------------------------------------------

    /** Method bodies follow the shared "Method bodies" setting, lambdas their own. */
    private fun blockCollapsed(block: ASTNode): Boolean = when (block.treeParent?.elementType) {
        E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
            CodeFoldingSettings.getInstance().COLLAPSE_METHODS
        E.LAMBDA_EXPRESSION -> JuxCodeFoldingSettings.getInstance().collapseLambdas
        else -> false
    }

    /** Whether [body] belongs to a type declared inside another type. */
    private fun isInnerClassBody(body: ASTNode): Boolean {
        val owner = body.treeParent?.psi as? JuxTypeDeclaration ?: return false
        return com.intellij.psi.util.PsiTreeUtil.getParentOfType(owner, JuxTypeDeclaration::class.java) != null
    }

    /** The range from the node's first `{` child to the node end, or null. */
    private fun braceRange(node: ASTNode): TextRange? {
        var child = node.firstChildNode
        while (child != null) {
            if (child.elementType === T.LBRACE) {
                return TextRange(child.startOffset, node.textRange.endOffset)
            }
            child = child.treeNext
        }
        return null
    }

    // ---- strings -----------------------------------------------------------

    /**
     * A long one-line literal keeps its opening and folds the rest:
     * `"Lorem ipsum dolor sit am..."`.
     */
    private fun foldLongString(node: ASTNode, out: MutableList<FoldingDescriptor>) {
        if (node.text.contains('\n')) return
        val start = node.startOffset + STRING_KEEP
        val range = TextRange(start, node.textRange.endOffset)
        out.add(descriptor(node, range, "...\"", JuxCodeFoldingSettings.getInstance().collapseLongStrings))
    }

    private fun isMultiline(range: TextRange, document: Document): Boolean =
        range.endOffset <= document.textLength &&
            document.getLineNumber(range.startOffset) < document.getLineNumber(range.endOffset)

    // Placeholders and collapse states travel on each descriptor; these are
    // only asked for descriptors that carry none, which this builder never makes.
    override fun getPlaceholderText(node: ASTNode): String = "..."

    override fun isCollapsedByDefault(node: ASTNode): Boolean = false

    private companion object {
        val SWITCHES = setOf(E.SWITCH_STATEMENT, E.SWITCH_EXPRESSION)
        val RAW_STRINGS = setOf(T.RAW_STRING_LITERAL, T.INTERP_RAW_STRING_LITERAL)
        val STRINGS = setOf(T.STRING_LITERAL, T.INTERP_STRING_LITERAL)
        val COMMENTS = setOf(T.LINE_COMMENT, T.BLOCK_COMMENT, T.DOC_COMMENT)

        /** A literal longer than this folds. */
        const val LONG_STRING = 80

        /** How much of a long literal stays visible, its quote included. */
        const val STRING_KEEP = 40

        /** The longest documentation summary a folded doc comment shows. */
        const val DOC_SUMMARY = 60
    }
}
