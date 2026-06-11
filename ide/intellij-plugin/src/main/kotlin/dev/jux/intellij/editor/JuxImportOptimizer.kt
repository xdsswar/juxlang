package dev.jux.intellij.editor

import com.intellij.lang.ImportOptimizer
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
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
 * The unused/duplicate analysis lives in [JuxImportSupport] (shared with the
 * unused-import inspection). The rewrite is purely textual over the document
 * so it needs no element factory, and it bails out (does nothing) if anything
 * other than whitespace sits between the imports — never eating an
 * interleaved comment.
 *
 * Registered via `<lang.importOptimizer>` in `plugin.xml`; the `Ctrl+Alt+O`
 * binding is the platform default, so no keymap entry is required.
 */
class JuxImportOptimizer : ImportOptimizer {
    override fun supports(file: PsiFile): Boolean = file is JuxFile

    override fun processFile(file: PsiFile): Runnable {
        // All analysis happens up front (read context); the returned Runnable
        // only mutates the document (write context).
        val imports = JuxImportSupport.collectImports(file)
        if (imports.isEmpty()) return EMPTY

        // Names referenced anywhere outside the import region.
        val used = JuxImportSupport.collectUsedNames(file, imports)

        // Filter unused / duplicate, preserving the first occurrence of each.
        val seen = HashSet<String>()
        val kept = ArrayList<JuxImportSupport.ImportInfo>()
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
    }
}
