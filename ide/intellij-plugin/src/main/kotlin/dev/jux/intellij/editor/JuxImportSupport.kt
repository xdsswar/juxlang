package dev.jux.intellij.editor

import com.intellij.openapi.project.DumbService
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.highlight.JuxKeywords
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.inspections.JuxUnresolvedReferenceInspection
import dev.jux.intellij.resolve.JuxDeclarationIndex
import dev.jux.intellij.resolve.JuxReference
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Shared import analysis — the single source of truth for "what does this
 * import bind, and is it used?". Consumed by both the Optimize Imports action
 * ([JuxImportOptimizer]) and the unused-import inspection, so the two can
 * never disagree about what counts as unused.
 */
object JuxImportSupport {

    /** One member of a grouped import `a.b.{ X, Y as Z }`. */
    class GroupItem(
        /** The member as written, `Y as Z`. */
        val text: String,
        /** The name it binds, `Z`. */
        val boundName: String,
        /** Where it sits inside the statement's text. */
        val rangeInStatement: TextRange,
    )

    /** One import statement, distilled to what optimization/inspection needs. */
    class ImportInfo(
        val element: PsiElement,
        val text: String,
        /** The dotted path before any `*`, `{ … }` or `as`: `a.b` of `import a.b.*;`. */
        val path: String,
        val sortKey: String,
        val dedupKey: String,
        val boundNames: Set<String>,
        /** `import a.b.*;` */
        val wildcard: Boolean,
        /** The members of a grouped import; empty for any other form. */
        val items: List<GroupItem>,
    ) {
        /** Kept for the callers that only ask "can this be judged at all?". */
        val alwaysKeep: Boolean get() = wildcard
    }

    /** What Optimize Imports does with an import, and what the inspection says about it. */
    enum class Verdict {
        /** Used as written. */
        KEEP,

        /** Binds nothing the file uses. */
        UNUSED,

        /** An exact repeat of an earlier import. */
        DUPLICATE,

        /** A grouped import with some members unused: keep the rest. */
        PRUNE,
    }

    class Decision(
        val import: ImportInfo,
        val verdict: Verdict,
        /** For [Verdict.PRUNE], the members that stay. */
        val keptItems: List<GroupItem> = emptyList(),
        /** For [Verdict.PRUNE], the members that go. */
        val droppedItems: List<GroupItem> = emptyList(),
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
     * Decide every import of [file] at once.
     *
     * A single-name or aliased import is used when its name is; a grouped one
     * is judged member by member; a duplicate always goes. A wildcard is
     * dropped only when it is PROVABLY unused ([wildcardMaySupply]): every
     * name the file takes from package scope is declared, according to the
     * project index, somewhere other than that package. Anything the index
     * cannot place keeps the wildcard, since it might be where the name comes
     * from.
     */
    fun analyze(file: PsiFile): List<Decision> {
        val imports = collectImports(file)
        if (imports.isEmpty()) return emptyList()
        val used = collectUsedNames(file, imports)
        val seen = HashSet<String>()
        // Names that must come from package scope: referenced by bare name,
        // not introduced in this file, not bound by an explicit import.
        val fromPackageScope: Set<String> by lazy {
            val explicit = imports.filterNot { it.wildcard }.flatMap { it.boundNames }.toSet()
            bareReferenceNames(file) - collectDefinedNames(file) - explicit - ALWAYS_IN_SCOPE
        }
        return imports.map { imp ->
            when {
                !seen.add(imp.dedupKey) -> Decision(imp, Verdict.DUPLICATE)
                imp.wildcard ->
                    if (wildcardMaySupply(file, imp.path, fromPackageScope)) {
                        Decision(imp, Verdict.KEEP)
                    } else {
                        Decision(imp, Verdict.UNUSED)
                    }
                imp.items.isNotEmpty() -> {
                    val (kept, dropped) = imp.items.partition { it.boundName in used }
                    when {
                        kept.isEmpty() -> Decision(imp, Verdict.UNUSED)
                        dropped.isEmpty() -> Decision(imp, Verdict.KEEP)
                        else -> Decision(imp, Verdict.PRUNE, kept, dropped)
                    }
                }
                imp.boundNames.none { it in used } -> Decision(imp, Verdict.UNUSED)
                else -> Decision(imp, Verdict.KEEP)
            }
        }
    }

    /**
     * The text an import is rewritten to after pruning: the kept members of a
     * group, or a plain single import when only one is left.
     */
    fun prunedText(decision: Decision): String {
        val imp = decision.import
        val prefix = imp.text.substringBefore('{')
        val items = decision.keptItems.map { it.text }
        return if (items.size == 1) "$prefix${items.single()};" else "$prefix{${items.joinToString(", ")}};"
    }

    /**
     * Whether a wildcard import of [pkg] may supply any of [names].
     *
     * True as soon as one name is declared in [pkg], and also whenever the
     * index cannot answer -- while indexing, before the index has data, or for
     * a name it has never seen -- because keeping an unneeded wildcard costs a
     * line and removing a needed one breaks the file.
     */
    fun wildcardMaySupply(file: PsiFile, pkg: String, names: Set<String>): Boolean {
        val project = file.project
        if (DumbService.isDumb(project) || !JuxDeclarationIndex.hasData(project)) return true
        val scope = GlobalSearchScope.allScope(project)
        val psiManager = PsiManager.getInstance(project)
        for (name in names) {
            val files = JuxDeclarationIndex.containingFiles(name, project, scope)
            if (files.isEmpty()) return true
            for (vf in files) {
                val psi = psiManager.findFile(vf) ?: return true
                if (JuxAutoImport.packageOfFile(psi) == pkg) return true
            }
        }
        return false
    }

    /**
     * All identifier texts referenced outside the import region — the usage
     * set an import must intersect to survive. Package and import statements
     * are skipped so an import never counts as its own use. Interpolated
     * strings are one lexer token, so names used only inside their `${…}`
     * holes (or `$name` shorthand) are extracted from the token text — a type
     * referenced exclusively inside an interpolation must keep its import.
     */
    fun collectUsedNames(file: PsiFile, imports: List<ImportInfo>): Set<String> {
        val importNodes = imports.map { it.element }.toHashSet()
        val used = HashSet<String>()
        fun walk(node: PsiElement) {
            val type = node.elementType
            if (type === E.IMPORT_STATEMENT || type === E.PACKAGE_STATEMENT) return
            when (type) {
                JuxTokenTypes.IDENTIFIER -> used.add(node.text)
                JuxTokenTypes.INTERP_STRING_LITERAL ->
                    used.addAll(interpolatedNames(node.text, raw = false))
                JuxTokenTypes.INTERP_RAW_STRING_LITERAL ->
                    used.addAll(interpolatedNames(node.text, raw = true))
            }
            var child = node.firstChild
            while (child != null) {
                if (child !in importNodes) walk(child)
                child = child.nextSibling
            }
        }
        walk(file)
        return used
    }

    /**
     * Every name introduced as a binding in [file]: an identifier in any
     * position that is not a USE (declarations, parameters, loop, catch and
     * lambda variables, patterns). One rule covers every binding construct.
     */
    fun collectDefinedNames(file: PsiFile): Set<String> {
        val names = HashSet<String>()
        PsiTreeUtil.processElements(file) { e ->
            if (e.elementType === JuxTokenTypes.IDENTIFIER &&
                e.parent?.elementType !in JuxUnresolvedReferenceInspection.USE_PARENTS
            ) {
                names.add(e.text)
            }
            true
        }
        return names
    }

    /**
     * Names referenced BARE -- a value or type reference with no qualifier.
     * A member access (`obj.x`) resolves through its receiver, never through
     * an import, so it is not collected. Interpolation holes are included.
     */
    private fun bareReferenceNames(file: PsiFile): Set<String> {
        val names = HashSet<String>()
        PsiTreeUtil.processElements(file) { e ->
            // A package or import statement names packages, not things the
            // file uses.
            if (inPackageOrImport(e)) return@processElements true
            if (e.elementType === E.REFERENCE_EXPRESSION || e.elementType === E.TYPE_REFERENCE) {
                (e.references.firstOrNull() as? JuxReference)?.value?.let { names.add(it) }
            }
            when (e.elementType) {
                JuxTokenTypes.INTERP_STRING_LITERAL -> names.addAll(interpolatedNames(e.text, raw = false))
                JuxTokenTypes.INTERP_RAW_STRING_LITERAL -> names.addAll(interpolatedNames(e.text, raw = true))
            }
            true
        }
        return names
    }

    /** Whether [e] sits inside a `package` or `import` statement. */
    private fun inPackageOrImport(e: PsiElement): Boolean {
        var p: PsiElement? = e
        while (p != null && p !is PsiFile) {
            if (p.elementType === E.IMPORT_STATEMENT || p.elementType === E.PACKAGE_STATEMENT) return true
            p = p.parent
        }
        return false
    }

    /** Names in scope everywhere: they never come from an import. */
    private val ALWAYS_IN_SCOPE: Set<String> by lazy {
        JuxUnresolvedReferenceInspection.BUILTIN_NAMES + JuxKeywords.PRIMITIVES +
            JuxKeywords.CONSTANTS + JuxKeywords.KEYWORDS
    }

    /**
     * Identifier-shaped words interpolated inside a `$"…"` / `$"""…"""` token:
     * everything in `${…}` holes (depth-tracked) plus `$name` shorthand. In
     * the cooked form a backslash escapes the next char (`\$` is no hole); in
     * the raw form `\` is plain text and `\${x}` IS an active hole — matching
     * `juxc-parse`'s interpolation segmentation. Over-collection is fine: the
     * result feeds used-name / suppression sets where a false "used" is the
     * safe direction.
     */
    fun interpolatedNames(text: String, raw: Boolean): Set<String> {
        val names = HashSet<String>()
        var i = 0
        while (i < text.length) {
            val c = text[i]
            if (!raw && c == '\\') {
                i += 2
                continue
            }
            if (c == '$' && i + 1 < text.length) {
                val next = text[i + 1]
                if (next == '{') {
                    var depth = 1
                    var j = i + 2
                    val start = j
                    while (j < text.length && depth > 0) {
                        when (text[j]) {
                            '\\' -> if (!raw) j++ // escaped char inside the hole
                            '{' -> depth++
                            '}' -> depth--
                        }
                        j++
                    }
                    val end = (if (depth == 0) j - 1 else j).coerceIn(start, text.length)
                    IDENT.findAll(text.substring(start, end)).forEach { names.add(it.value) }
                    i = j
                    continue
                }
                if (next.isLetter() || next == '_') {
                    val m = IDENT.matchAt(text, i + 1)
                    if (m != null) names.add(m.value)
                }
            }
            i++
        }
        return names
    }

    private val IDENT = Regex("[A-Za-z_][A-Za-z0-9_]*")

    /** Extract the bound names and sort/dedup keys from one import statement. */
    private fun describe(stmt: PsiElement): ImportInfo {
        val text = stmt.text.trim()
        // The dotted path lives in the QUALIFIED_NAME child; the rest of the
        // statement carries the wildcard / brace-group / alias shape.
        val path = stmt.children.firstOrNull { it.elementType === E.QUALIFIED_NAME }?.text ?: ""

        val bound = LinkedHashSet<String>()
        val hasWildcard = text.contains('*')
        val items = if (text.contains('{')) groupItems(text) else emptyList()
        val alias = aliasName(stmt)

        when {
            hasWildcard -> Unit
            items.isNotEmpty() -> items.forEach { bound.add(it.boundName) }
            alias != null -> bound.add(alias)
            else -> path.substringAfterLast('.').takeIf { it.isNotEmpty() }?.let { bound.add(it) }
        }

        // Sort key: the path, then the whole text (so aliases of the same path
        // order stably). Dedup key: whitespace-collapsed full text.
        val sortKey = (path + " " + text).lowercase()
        val dedupKey = text.replace(WHITESPACE, " ")
        return ImportInfo(stmt, text, path.removeSuffix(".*"), sortKey, dedupKey, bound, hasWildcard, items)
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
     * The members of a grouped import `a.b.{ X, Y as Z }`, each with the name
     * it binds (the alias when one is present) and its range in the statement.
     * Parsed from the brace text since the group is consumed as raw tokens (no
     * IMPORT_ITEM nodes today).
     */
    private fun groupItems(text: String): List<GroupItem> {
        val open = text.indexOf('{')
        val close = text.lastIndexOf('}')
        if (open < 0 || close <= open) return emptyList()
        val result = ArrayList<GroupItem>()
        var start = open + 1
        while (start <= close) {
            var end = text.indexOf(',', start)
            if (end < 0 || end > close) end = close
            val raw = text.substring(start, end)
            val item = raw.trim()
            if (item.isNotEmpty() && item != "*") {
                val asIdx = item.indexOf(" as ")
                val name = if (asIdx >= 0) item.substring(asIdx + 4).trim() else item
                val itemStart = start + raw.indexOf(item)
                result.add(GroupItem(item, name, TextRange(itemStart, itemStart + item.length)))
            }
            start = end + 1
        }
        return result
    }

    val WHITESPACE = Regex("\\s+")
}
