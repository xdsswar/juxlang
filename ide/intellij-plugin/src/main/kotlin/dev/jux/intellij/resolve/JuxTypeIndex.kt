package dev.jux.intellij.resolve

import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Key
import com.intellij.psi.PsiElement
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

    /**
     * The type named [name] **as seen from [context]** — its own file first,
     * then the rest of the project.
     *
     * This is the overload nearly every caller wants. A Jux file is compiled
     * against its own imports, so the same bare name legitimately names
     * different types in different files: `examples/` alone has `Tagged` as an
     * interface in one file and as a class in two others. The project-wide
     * lookup returns whichever the platform happened to index first, so an
     * inspection could read a file's own `interface Tagged`, resolve the name
     * to an unrelated `class Tagged`, and report a confident error about code
     * that is correct ("Class 'Base' cannot implement 'Tagged' because it is a
     * class"). Every inspection, gutter and completion built on name
     * resolution inherited that.
     *
     * Preferring the enclosing file makes the single-file case exact and leaves
     * genuinely cross-file resolution — the workspace shape, where names do not
     * collide — on the project walk. Package-aware resolution across files is
     * the LSP's job; this is the IDE-side approximation.
     */
    fun findType(context: PsiElement, name: String): JuxTypeDeclaration? {
        val file = context.containingFile
        if (file != null) {
            for (decl in typesIn(file)) {
                if (decl.name == name) return decl
            }
        }
        return findType(context.project, name)
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
        // FileTypeIndex.getFiles throws IndexNotReadyException during indexing;
        // every caller (line markers, inspections, annotator, Go-to) runs on the
        // daemon/EDT where that surfaces as an error. Yield nothing while dumb —
        // the daemon re-runs once indexing completes.
        if (DumbService.isDumb(project)) return
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
        // See forEachType: skip while indexing so callers never hit IndexNotReadyException.
        if (DumbService.isDumb(project)) return
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
