package dev.jux.intellij.resolve

import com.intellij.openapi.project.Project
import com.intellij.psi.search.GlobalSearchScope
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * Reverse-hierarchy queries — "who extends / implements / overrides this?" —
 * shared by the down-arrow gutters ([JuxSubtypeLineMarkerProvider]) and Go-to
 * Implementation ([JuxImplementationSearch]). Resolution is project-wide via
 * [JuxTypeIndex] and by simple name, matching the rest of the IDE-side layer.
 *
 * The [buildIndex]/[transitiveSubtypes] split lets a batch caller (the gutter,
 * scanning a whole file) build the direct-subtype index ONCE and reuse it,
 * while the single-element callers get the convenience [subtypesOf] /
 * [overridingMethods] that build it on demand.
 */
object JuxSubtypes {

    /** supertype simple-name → the project types that directly name it. */
    fun buildIndex(project: Project): Map<String, List<JuxTypeDeclaration>> {
        val m = HashMap<String, MutableList<JuxTypeDeclaration>>()
        val all = ArrayList<JuxTypeDeclaration>()
        JuxTypeIndex.forEachType(project, GlobalSearchScope.allScope(project)) { all.add(it) }
        // `class Square implements Sh` with `type Sh = Shape;`, or with
        // `import app.Shape as Sh;`, is a subtype of `Shape` (ERRATA E133).
        // Only a name some `type` alias declares, or one an import of the
        // subtype's own file renames, is resolved, so the common case stays a
        // pure name walk.
        val aliasNames = all.filter { JuxTypeEngine.isTypeAlias(it) }.mapNotNullTo(HashSet()) { it.name }
        val importAliasesByFile = HashMap<com.intellij.psi.PsiFile, Set<String>>()
        for (t in all) {
            val file = t.containingFile
            val importAliases = if (file == null) emptySet() else importAliasesByFile.getOrPut(file) { importAliasNames(file) }
            for (sup in JuxHierarchy.superTypeNames(t)) {
                m.getOrPut(sup) { ArrayList() }.add(t)
                if (sup in aliasNames || sup in importAliases) {
                    val target = JuxTypeIndex.findTypeThroughAliases(t, sup)?.name
                    if (target != null && target != sup) m.getOrPut(target) { ArrayList() }.add(t)
                }
            }
        }
        return m
    }

    /** The names [file]'s imports bind under another simple name: `Z` of `import x.Y as Z;`. */
    private fun importAliasNames(file: com.intellij.psi.PsiFile): Set<String> =
        dev.jux.intellij.editor.JuxImportSupport.collectImports(file)
            .flatMap { it.targets.entries }
            .filter { (bound, fqn) -> fqn.substringAfterLast('.') != bound }
            .mapTo(HashSet()) { it.key }

    /** All transitive subtypes of [name] using a prebuilt [index]. */
    fun transitiveSubtypes(
        name: String,
        index: Map<String, List<JuxTypeDeclaration>>,
    ): List<JuxTypeDeclaration> {
        val out = LinkedHashSet<JuxTypeDeclaration>()
        val seen = HashSet<String>()
        val stack = ArrayDeque<String>()
        stack.addLast(name)
        while (stack.isNotEmpty()) {
            val n = stack.removeLast()
            if (!seen.add(n)) continue // cycle / diamond guard
            for (sub in index[n].orEmpty()) {
                if (out.add(sub)) sub.name?.let { stack.addLast(it) }
            }
        }
        return out.toList()
    }

    /** Methods in [type]'s subtypes that override [name]/[arity], via [index]. */
    fun overridingMethods(
        type: JuxTypeDeclaration,
        name: String,
        arity: Int,
        index: Map<String, List<JuxTypeDeclaration>>,
    ): List<JuxMethodDeclaration> {
        val ownerName = type.name ?: return emptyList()
        val out = ArrayList<JuxMethodDeclaration>()
        for (sub in transitiveSubtypes(ownerName, index)) {
            for (m in JuxHierarchy.directChildren(sub, E.METHOD_DECLARATION)) {
                // Same name + arity, and an actual override candidate: a static
                // method that merely shares the signature is not an override.
                if (m is JuxMethodDeclaration && m.name == name &&
                    JuxHierarchy.arity(m) == arity && !JuxHierarchy.hasModifier(m, "static")
                ) {
                    out.add(m)
                }
            }
        }
        return out
    }

    // ---- convenience (single element; builds the index on demand) -------------

    fun subtypesOf(type: JuxTypeDeclaration): List<JuxTypeDeclaration> {
        val name = type.name ?: return emptyList()
        return transitiveSubtypes(name, buildIndex(type.project))
    }

    /**
     * The types that name [type] directly in their own extends/implements
     * clause, using a prebuilt [index].
     *
     * The direct step, not the transitive closure: a hierarchy tree expands one
     * level at a time, so listing every descendant as a child of the root would
     * show `Circle` twice — once under `Shape`, where it belongs, and once
     * beside it.
     */
    fun directSubtypes(
        type: JuxTypeDeclaration,
        index: Map<String, List<JuxTypeDeclaration>>,
    ): List<JuxTypeDeclaration> {
        val name = type.name ?: return emptyList()
        return index[name].orEmpty()
    }

    fun overridingMethods(method: JuxMethodDeclaration): List<JuxMethodDeclaration> {
        val owner = JuxHierarchy.enclosingType(method) ?: return emptyList()
        val name = method.name ?: return emptyList()
        return overridingMethods(owner, name, JuxHierarchy.arity(method), buildIndex(method.project))
    }
}
