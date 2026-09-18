package dev.jux.intellij.editor

import com.intellij.lang.ImportOptimizer
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * **Optimize Imports** (`Ctrl+Alt+O`) for Jux, doing what IntelliJ's Java
 * optimizer does:
 *
 *  - drops imports whose bound name is never referenced in the file, and
 *    exact duplicates;
 *  - prunes the unused members of a grouped import (`import a.{X, Y};` with
 *    only `X` used becomes `import a.X;`);
 *  - drops a wildcard import the project index proves nothing is taken from
 *    ([JuxImportSupport.wildcardMaySupply]);
 *  - lays the survivors out in groups, alphabetical within each, separated by
 *    a blank line: bound crates (`rust.` / `c.` / `cpp.`), then the Jux
 *    library (`jux.`), then the project's own packages. That is the order the
 *    examples use, and the analogue of Java's default layout keeping library
 *    and project imports apart.
 *
 * The decisions live in [JuxImportSupport.analyze], shared with the
 * unused-import inspection so the two never disagree. The rewrite is textual
 * over the document, and it does nothing if anything other than whitespace
 * sits between the imports, so an interleaved comment is never eaten.
 *
 * Registered via `<lang.importOptimizer>` in `plugin.xml`; `Ctrl+Alt+O` is the
 * platform default binding, and the platform also runs it over directories and
 * on commit.
 */
class JuxImportOptimizer : ImportOptimizer {
    override fun supports(file: PsiFile): Boolean = file is JuxFile

    override fun processFile(file: PsiFile): Runnable {
        // All analysis happens up front (read context); the returned Runnable
        // only mutates the document (write context).
        val decisions = JuxImportSupport.analyze(file)
        if (decisions.isEmpty()) return EMPTY

        val kept = decisions.mapNotNull { d ->
            when (d.verdict) {
                JuxImportSupport.Verdict.KEEP -> d.import.sortKey to d.import.text
                JuxImportSupport.Verdict.PRUNE -> d.import.sortKey to JuxImportSupport.prunedText(d)
                JuxImportSupport.Verdict.UNUSED, JuxImportSupport.Verdict.DUPLICATE -> null
            }
        }

        val newBlock = layout(kept)

        // The contiguous span the imports occupy, plus a guard that nothing but
        // whitespace lives between them (so comments are never swallowed).
        val first = decisions.first().import.element
        val last = decisions.last().import.element
        if (!onlyWhitespaceBetween(first, last)) return EMPTY

        val start = first.textRange.startOffset
        val end = last.textRange.endOffset
        val oldBlock = file.text.substring(start, end)
        if (newBlock == oldBlock) return EMPTY // already optimal: no-op

        return Runnable {
            val docMgr = PsiDocumentManager.getInstance(file.project)
            val doc = docMgr.getDocument(file) ?: return@Runnable
            // An empty block leaves the blank line that followed it; take the
            // line break with the imports so the file does not open with a gap.
            var removeEnd = end
            if (newBlock.isEmpty()) {
                val text = doc.charsSequence
                while (removeEnd < text.length && (text[removeEnd] == '\n' || text[removeEnd] == '\r')) {
                    removeEnd++
                }
            }
            doc.replaceString(start, removeEnd, newBlock)
            docMgr.commitDocument(doc)
        }
    }

    /**
     * Group, sort and join the surviving import lines. Groups are separated by
     * one blank line; empty groups leave no trace.
     */
    private fun layout(lines: List<Pair<String, String>>): String =
        lines
            .groupBy { (_, text) -> groupOf(text) }
            .toSortedMap()
            .values
            .joinToString("\n\n") { group -> group.sortedBy { it.first }.joinToString("\n") { it.second } }

    /** 0: bound crates, 1: the Jux library, 2: everything else (the project). */
    private fun groupOf(importText: String): Int {
        val path = importText.removePrefix("import").trim()
        return when {
            path.startsWith("rust.") || path.startsWith("c.") || path.startsWith("cpp.") -> 0
            path.startsWith("jux.") -> 1
            else -> 2
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
