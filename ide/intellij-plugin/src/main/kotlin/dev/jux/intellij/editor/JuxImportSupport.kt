package dev.jux.intellij.editor

import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Shared import analysis — the single source of truth for "what does this
 * import bind, and is it used?". Consumed by both the Optimize Imports action
 * ([JuxImportOptimizer]) and the unused-import inspection, so the two can
 * never disagree about what counts as unused.
 */
object JuxImportSupport {

    /** One import statement, distilled to what optimization/inspection needs. */
    class ImportInfo(
        val element: PsiElement,
        val text: String,
        val sortKey: String,
        val dedupKey: String,
        val boundNames: Set<String>,
        val alwaysKeep: Boolean,
    )

    /** Gather every top-level `import` statement in source order. */
    fun collectImports(file: PsiFile): List<ImportInfo> {
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

    /**
     * All identifier texts referenced outside the import region — the usage
     * set an import must intersect to survive. Package and import statements
     * are skipped so an import never counts as its own use.
     */
    fun collectUsedNames(file: PsiFile, imports: List<ImportInfo>): Set<String> {
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

    /** Extract the bound names and sort/dedup keys from one import statement. */
    private fun describe(stmt: PsiElement): ImportInfo {
        val text = stmt.text.trim()
        // The dotted path lives in the QUALIFIED_NAME child; the rest of the
        // statement carries the wildcard / brace-group / alias shape.
        val path = stmt.children.firstOrNull { it.elementType === E.QUALIFIED_NAME }?.text ?: ""

        var alwaysKeep = false
        val bound = LinkedHashSet<String>()

        // Classify: `*` → wildcard, `{ … }` → group, `as X` → alias.
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
     * Bound names of a grouped import `a.b.{ X, Y as Z }`: the alias when one
     * is present, else the item's own name. Parsed from the brace text since
     * the group is consumed as raw tokens (no IMPORT_ITEM nodes today).
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

    val WHITESPACE = Regex("\\s+")
}
