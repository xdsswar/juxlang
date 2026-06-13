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
        JuxTypeIndex.forEachType(project, GlobalSearchScope.allScope(project)) { t ->
            for (sup in JuxHierarchy.superTypeNames(t)) {
                m.getOrPut(sup) { ArrayList() }.add(t)
            }
        }
        return m
    }

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
                if (m is JuxMethodDeclaration && m.name == name && JuxHierarchy.arity(m) == arity) {
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

    fun overridingMethods(method: JuxMethodDeclaration): List<JuxMethodDeclaration> {
        val owner = JuxHierarchy.enclosingType(method) ?: return emptyList()
        val name = method.name ?: return emptyList()
        return overridingMethods(owner, name, JuxHierarchy.arity(method), buildIndex(method.project))
    }
}
