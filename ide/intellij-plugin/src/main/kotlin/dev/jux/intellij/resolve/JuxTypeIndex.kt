package dev.jux.intellij.resolve

import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Key
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.FileTypeIndex
import com.intellij.psi.util.CachedValue
import com.intellij.psi.util.CachedValueProvider
import com.intellij.psi.util.CachedValuesManager
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * A lightweight, project-wide index of Jux type declarations — the cross-file
 * resolution the per-file [JuxReference] lacks. Backed by the platform's
 * [FileTypeIndex] (every `.jux` file in the project) rather than a custom stub
 * index. Per-file declaration lists are cached ([CachedValuesManager], keyed on
 * each file's modification stamp) because the override gutters and the
 * missing-override inspection call [findType] for every method on every daemon
 * pass — re-walking every file's full PSI each time scaled as
 * O(methods × supertypes × project files).
 */
object JuxTypeIndex {
    @PublishedApi
    internal val FILE_TYPES_KEY: Key<CachedValue<List<JuxTypeDeclaration>>> =
        Key.create("jux.file.type.declarations")

    /** The file's type declarations, cached until the file changes. */
    @PublishedApi
    internal fun typesIn(psi: PsiFile): List<JuxTypeDeclaration> =
        CachedValuesManager.getManager(psi.project).getCachedValue(psi, FILE_TYPES_KEY, {
            CachedValueProvider.Result.create(
                PsiTreeUtil.findChildrenOfType(psi, JuxTypeDeclaration::class.java).toList(),
                psi,
            )
        }, false)

    /** The first top-level (or nested) type named [name] anywhere in the project. */
    fun findType(project: Project, name: String): JuxTypeDeclaration? {
        forEachType(project) { if (it.name == name) return it }
        return null
    }

    /** Bare names of every declared type in the project (for completion). */
    fun allTypeNames(project: Project): List<String> {
        val out = LinkedHashSet<String>()
        forEachType(project) { it.name?.let(out::add) }
        return out.toList()
    }

    /**
     * Visits every type declaration in [scope] (Go-to-Class and friends).
     * Public, scope-aware variant of the internal walk.
     */
    inline fun forEachType(
        project: Project,
        scope: GlobalSearchScope,
        action: (JuxTypeDeclaration) -> Unit,
    ) {
        val manager = PsiManager.getInstance(project)
        for (vf in FileTypeIndex.getFiles(JuxFileType, scope)) {
            val psi = manager.findFile(vf) ?: continue
            for (decl in typesIn(psi)) {
                action(decl)
            }
        }
    }

    /**
     * Visits every named declaration in [scope] — types, methods, fields,
     * enum constants — for Go-to-Symbol. Parameters and locals are skipped:
     * symbol search is about declarations worth jumping to from anywhere.
     */
    inline fun forEachSymbol(
        project: Project,
        scope: GlobalSearchScope,
        action: (JuxNamedElement) -> Unit,
    ) {
        val manager = PsiManager.getInstance(project)
        for (vf in FileTypeIndex.getFiles(JuxFileType, scope)) {
            val psi = manager.findFile(vf) ?: continue
            for (decl in PsiTreeUtil.findChildrenOfType(psi, JuxNamedElement::class.java)) {
                if (decl is dev.jux.intellij.psi.JuxParameter ||
                    decl is dev.jux.intellij.psi.JuxLocalVariable
                ) continue
                action(decl)
            }
        }
    }

    private inline fun forEachType(project: Project, action: (JuxTypeDeclaration) -> Unit) =
        forEachType(project, GlobalSearchScope.allScope(project), action)
}
