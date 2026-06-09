package dev.jux.intellij.editor

import com.intellij.lang.ImportOptimizer
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Java-like **Optimize Imports** (`Ctrl+Alt+O`) for Jux.
 *
 * Walks the file's `import` statements and, in one write action:
 *  - drops imports whose bound name is never referenced in the file,
 *  - drops exact duplicates,
 *  - sorts the survivors alphabetically by their import path.
 *
 * Wildcard imports (`import a.b.*`) and side-effect-only forms are always kept
 * (their usage can't be proven from this file alone). Grouped imports
 * (`import a.b.{X, Y as Z}`) survive if *any* of their bound names is used.
 *
 * The rewrite is purely textual over the document so it needs no element
 * factory, and it bails out (does nothing) if anything other than whitespace
 * sits between the imports — never eating an interleaved comment.
 *
 * Registered via `<lang.importOptimizer>` in `plugin.xml`; the `Ctrl+Alt+O`
 * binding is the platform default, so no keymap entry is required.
 */
class JuxImportOptimizer : ImportOptimizer {
    override fun supports(file: PsiFile): Boolean = file is JuxFile

    override fun processFile(file: PsiFile): Runnable {
        // All analysis happens up front (read context); the returned Runnable
        // only mutates the document (write context).
        val imports = collectImports(file)
        if (imports.isEmpty()) return EMPTY

        // Names referenced anywhere outside the import region.
        val used = collectUsedNames(file, imports)

        // Filter unused / duplicate, preserving the first occurrence of each.
        val seen = HashSet<String>()
        val kept = ArrayList<ImportInfo>()
        for (imp in imports) {
            if (!seen.add(imp.dedupKey)) continue // exact duplicate
            if (!imp.alwaysKeep && imp.boundNames.none { it in used }) continue // unused
            kept.add(imp)
        }

        // Java orders imports alphabetically by their path text.
        val sorted = kept.sortedBy { it.sortKey }

        // The contiguous span the imports occupy, plus a guard that nothing but
        // whitespace lives between them (so comments are never swallowed).
        val first = imports.first().element
        val last = imports.last().element
        if (!onlyWhitespaceBetween(first, last)) return EMPTY

        val newBlock = sorted.joinToString("\n") { it.text }
        val oldBlock = file.text.substring(first.textRange.startOffset, last.textRange.endOffset)
        if (newBlock == oldBlock) return EMPTY // already optimal — no-op

        val start = first.textRange.startOffset
        val end = last.textRange.endOffset
        return Runnable {
            val docMgr = PsiDocumentManager.getInstance(file.project)
            val doc = docMgr.getDocument(file) ?: return@Runnable
            doc.replaceString(start, end, newBlock)
            docMgr.commitDocument(doc)
        }
    }

    // ---- analysis ---------------------------------------------------------

    /** One import statement, distilled to what optimization needs. */
    private class ImportInfo(
        val element: PsiElement,
        val text: String,
        val sortKey: String,
        val dedupKey: String,
        val boundNames: Set<String>,
        val alwaysKeep: Boolean,
    )

    /** Gather every top-level `import` statement in source order. */
    private fun collectImports(file: PsiFile): List<ImportInfo> {
        val result = ArrayList<ImportInfo>()
        var child = file.firstChild
        while (child != null) {
            if (child.elementType === E.IMPORT_STATEMENT) {
                result.add(describe(child))
            }
            child = child.nextSibling
        }
        return result
    }

    /** Extract the bound names and sort/dedup keys from one import statement. */
    private fun describe(stmt: PsiElement): ImportInfo {
        val text = stmt.text.trim()
        // The dotted path lives in the QUALIFIED_NAME child; the rest of the
        // statement carries the wildcard / brace-group / alias shape.
        val path = stmt.children.firstOrNull { it.elementType === E.QUALIFIED_NAME }?.text ?: ""

        var alwaysKeep = false
        val bound = LinkedHashSet<String>()

        // Walk leaves to classify: `*` → wildcard, `{ … }` → group, `as X` → alias.
        val hasWildcard = stmt.text.contains('*')
        val hasBrace = stmt.text.contains('{')
        val alias = aliasName(stmt)

        when {
            hasWildcard -> alwaysKeep = true
            hasBrace -> bound.addAll(groupBoundNames(stmt))
            alias != null -> bound.add(alias)
            else -> path.substringAfterLast('.').takeIf { it.isNotEmpty() }?.let { bound.add(it) }
        }

        // Sort key: the path, then the whole text (so aliases of the same path
        // order stably). Dedup key: whitespace-collapsed full text.
        val sortKey = (path + " " + text).lowercase()
        val dedupKey = text.replace(WHITESPACE, " ")
        return ImportInfo(stmt, text, sortKey, dedupKey, bound, alwaysKeep)
    }

    /** The identifier after a trailing `as` in `import a.b.C as D`, or null. */
    private fun aliasName(stmt: PsiElement): String? {
        var child = stmt.firstChild
        var sawAs = false
        while (child != null) {
            when {
                child.elementType === JuxTokenTypes.AS_KW -> sawAs = true
                sawAs && child.elementType === JuxTokenTypes.IDENTIFIER -> return child.text
            }
            child = child.nextSibling
        }
        return null
    }

    /**
     * Bound names of a grouped import `a.b.{ X, Y as Z }`: the alias when one is
     * present, else the item's own name. Parsed from the brace text since the
     * group is consumed as raw tokens (no IMPORT_ITEM nodes today).
     */
    private fun groupBoundNames(stmt: PsiElement): Set<String> {
        val body = stmt.text.substringAfter('{', "").substringBeforeLast('}', "")
        if (body.isEmpty()) return emptySet()
        val names = LinkedHashSet<String>()
        for (raw in body.split(',')) {
            val item = raw.trim()
            if (item.isEmpty()) continue
            val asIdx = item.indexOf(" as ")
            val name = if (asIdx >= 0) item.substring(asIdx + 4).trim() else item
            name.takeIf { it.isNotEmpty() && it != "*" }?.let { names.add(it) }
        }
        return names
    }

    /**
     * All identifier texts referenced outside the import region — the usage set
     * an import must intersect to survive. Package and import statements are
     * skipped so an import never counts as its own use.
     */
    private fun collectUsedNames(file: PsiFile, imports: List<ImportInfo>): Set<String> {
        val importNodes = imports.map { it.element }.toHashSet()
        val used = HashSet<String>()
        fun walk(node: PsiElement) {
            val type = node.elementType
            if (type === E.IMPORT_STATEMENT || type === E.PACKAGE_STATEMENT) return
            if (type === JuxTokenTypes.IDENTIFIER) used.add(node.text)
            var child = node.firstChild
            while (child != null) {
                if (child !in importNodes) walk(child)
                child = child.nextSibling
            }
        }
        walk(file)
        return used
    }

    /** True if only whitespace separates the two (sibling) elements. */
    private fun onlyWhitespaceBetween(first: PsiElement, last: PsiElement): Boolean {
        var node: PsiElement? = first
        while (node != null && node !== last) {
            val next = node.nextSibling ?: return false
            if (next !== last && next.elementType !== E.IMPORT_STATEMENT &&
                next.text.isNotBlank()
            ) {
                return false
            }
            node = next
        }
        return true
    }

    private companion object {
        val EMPTY = Runnable {}
        val WHITESPACE = Regex("\\s+")
    }
}
