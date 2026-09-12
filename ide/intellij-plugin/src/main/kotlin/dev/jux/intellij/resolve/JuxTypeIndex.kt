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
import com.intellij.psi.util.PsiModificationTracker
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.editor.JuxImportSupport
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

    @PublishedApi
    internal val FILE_SYMBOLS_KEY: Key<CachedValue<List<JuxNamedElement>>> =
        Key.create("jux.file.named.symbols")

    @PublishedApi
    internal val FILE_NAMES_KEY: Key<CachedValue<Set<String>>> =
        Key.create("jux.file.declared.names")

    private val TYPES_BY_NAME_KEY: Key<CachedValue<Map<String, List<JuxTypeDeclaration>>>> =
        Key.create("jux.project.types.by.name")

    /** The file's type declarations, cached until the file changes. */
    @PublishedApi
    internal fun typesIn(psi: PsiFile): List<JuxTypeDeclaration> =
        CachedValuesManager.getManager(psi.project).getCachedValue(psi, FILE_TYPES_KEY, {
            CachedValueProvider.Result.create(
                PsiTreeUtil.findChildrenOfType(psi, JuxTypeDeclaration::class.java).toList(),
                psi,
            )
        }, false)

    /**
     * The file's named declarations -- types, methods, fields, enum constants
     * -- cached until the file changes.
     *
     * Parameters and locals are skipped: a symbol worth indexing project-wide
     * is one worth jumping to from another file, and a loop variable is not.
     */
    @PublishedApi
    internal fun symbolsIn(psi: PsiFile): List<JuxNamedElement> =
        CachedValuesManager.getManager(psi.project).getCachedValue(psi, FILE_SYMBOLS_KEY, {
            val out = PsiTreeUtil.findChildrenOfType(psi, JuxNamedElement::class.java)
                .filterNot {
                    it is dev.jux.intellij.psi.JuxParameter ||
                        it is dev.jux.intellij.psi.JuxLocalVariable
                }
            CachedValueProvider.Result.create(out, psi)
        }, false)

    /** Just the names from [symbolsIn], cached separately so the common
     *  "does this name exist anywhere" question costs a map lookup. */
    @PublishedApi
    internal fun namesIn(psi: PsiFile): Set<String> =
        CachedValuesManager.getManager(psi.project).getCachedValue(psi, FILE_NAMES_KEY, {
            val names = HashSet<String>()
            for (d in symbolsIn(psi)) d.name?.let(names::add)
            CachedValueProvider.Result.create(names as Set<String>, psi)
        }, false)

    /**
     * Every declared name in [scope], as one set.
     *
     * Answered from [JuxDeclarationIndex], which the platform maintains
     * incrementally and persists: no file's PSI is loaded, and a keystroke
     * re-indexes only the file that changed. The inspection that asks this
     * runs on every daemon pass, so this is the one lookup most worth
     * keeping off the PSI path.
     *
     * The per-file walk remains as a fallback for when the index has no
     * answer to give -- a fixture with no index, or an environment where it
     * has not been built. It is the same code that answered before the index
     * existed, so the fallback is a slower answer rather than a missing one.
     */
    fun declaredNames(project: Project, scope: GlobalSearchScope): Set<String> {
        if (DumbService.isDumb(project)) return emptySet()
        return walkDeclaredNames(project, scope)
    }

    /**
     * "Is this name declared anywhere in the project?", ready to be asked
     * many times.
     *
     * Resolved once per call: the index when it has data, the PSI walk when
     * it does not -- and the walk is `lazy`, so a project with a working
     * index never builds it.
     *
     * **While the IDE is indexing the answer is yes, for everything.** The
     * honest answer is "unknown", and of the two ways to be wrong, claiming a
     * name is undeclared paints red over correct code until indexing
     * finishes. Silence is the right failure.
     */
    fun projectNamePredicate(project: Project): (String) -> Boolean {
        if (DumbService.isDumb(project)) return { true }
        val scope = GlobalSearchScope.allScope(project)
        if (JuxDeclarationIndex.hasData(project)) {
            return { name -> JuxDeclarationIndex.isDeclared(name, project, scope) }
        }
        val walked = lazy { walkDeclaredNames(project, scope) }
        return { name -> name in walked.value }
    }

    /** The pre-index answer: every file's cached name set, unioned. */
    private fun walkDeclaredNames(project: Project, scope: GlobalSearchScope): Set<String> {
        val manager = PsiManager.getInstance(project)
        val names = HashSet<String>()
        for (vf in FileTypeIndex.getFiles(JuxFileType, scope)) {
            val psi = manager.findFile(vf) ?: continue
            names.addAll(namesIn(psi))
        }
        return names
    }

    /**
     * Every type declaration in the project, grouped by bare name.
     *
     * Rebuilt whenever anything in the project changes -- but from the
     * per-file caches, so the rebuild merges lists rather than walking PSI.
     * What it buys is the lookup: [findType] used to scan files until it met
     * the name, and its callers ask once per method per supertype on every
     * daemon pass.
     */
    private fun typesByName(project: Project): Map<String, List<JuxTypeDeclaration>> =
        CachedValuesManager.getManager(project).getCachedValue(project, TYPES_BY_NAME_KEY, {
            val map = HashMap<String, MutableList<JuxTypeDeclaration>>()
            if (!DumbService.isDumb(project)) {
                val manager = PsiManager.getInstance(project)
                for (vf in FileTypeIndex.getFiles(JuxFileType, GlobalSearchScope.allScope(project))) {
                    val psi = manager.findFile(vf) ?: continue
                    for (decl in typesIn(psi)) {
                        val n = decl.name ?: continue
                        map.getOrPut(n) { mutableListOf() }.add(decl)
                    }
                }
            }
            CachedValueProvider.Result.create(
                map as Map<String, List<JuxTypeDeclaration>>,
                PsiModificationTracker.MODIFICATION_COUNT,
            )
        }, false)

    /**
     * The first top-level (or nested) type named [name] anywhere in the
     * project.
     *
     * "First" is the first declaration the file walk met, which is what the
     * previous scan returned too -- several files may legally declare the
     * same bare name, and choosing between them is the caller's business
     * (see `findType(context, name)`, which prefers the context's own file).
     */
    fun findType(project: Project, name: String): JuxTypeDeclaration? {
        // Ask the index which files declare the name, and parse only those.
        // On a project of any size this is the difference between touching
        // one file and touching all of them.
        if (!DumbService.isDumb(project)) {
            val scope = GlobalSearchScope.allScope(project)
            val manager = PsiManager.getInstance(project)
            for (vf in JuxDeclarationIndex.containingFiles(name, project, scope)) {
                val psi = manager.findFile(vf) ?: continue
                typesIn(psi).firstOrNull { it.name == name }?.let { return it }
            }
        }
        // No index (or nothing in it): the project-wide map, which is still
        // cheaper than the scan this replaced.
        return typesByName(project)[name]?.firstOrNull()
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
            ?: return findType(context.project, name)

        // 1. The file's own declarations. A name declared here means here.
        for (decl in typesIn(file)) {
            if (decl.name == name) return decl
        }

        val candidates = candidateTypes(context.project, name)
        if (candidates.size == 1) return candidates.first()
        if (candidates.isEmpty()) return null

        // 2. An explicit import decides between same-named types. Two
        //    `Animal`s in one project is not a mistake, and the file already
        //    says which one it means -- reading `import poll.lib.Animal;` is
        //    the difference between a correct answer and a coin toss.
        val importedPackage = importedPackageFor(file, name)
        if (importedPackage != null) {
            candidates.firstOrNull { JuxAutoImport.packageOf(it) == importedPackage }
                ?.let { return it }
        }

        // 3. A sibling in the same package needs no import, so it wins next.
        val ownPackage = JuxAutoImport.packageOfFile(file)
        candidates.firstOrNull { JuxAutoImport.packageOf(it) == ownPackage }
            ?.let { return it }

        // 4. Nothing distinguishes them: any answer is a guess, so give the
        //    first and let the caller be as wrong as the source is ambiguous.
        return candidates.first()
    }

    /**
     * Every type named [name] in the project.
     *
     * Through the index where there is one, so only the files that actually
     * declare the name are parsed.
     */
    private fun candidateTypes(project: Project, name: String): List<JuxTypeDeclaration> {
        if (!DumbService.isDumb(project) && JuxDeclarationIndex.hasData(project)) {
            val manager = PsiManager.getInstance(project)
            val out = ArrayList<JuxTypeDeclaration>()
            val scope = GlobalSearchScope.allScope(project)
            for (vf in JuxDeclarationIndex.containingFiles(name, project, scope)) {
                val psi = manager.findFile(vf) ?: continue
                out.addAll(typesIn(psi).filter { it.name == name })
            }
            if (out.isNotEmpty()) return out
        }
        return typesByName(project)[name].orEmpty()
    }

    /**
     * The package an explicit `import <pkg>.<name>;` in [file] binds [name]
     * to, or null when the file imports it under no package (or not at all).
     *
     * Wildcard imports are deliberately not consulted: they bind whatever is
     * there, so they cannot break a tie between two candidates.
     */
    private fun importedPackageFor(file: PsiFile, name: String): String? {
        for (import in JuxImportSupport.collectImports(file)) {
            if (import.alwaysKeep) continue // a wildcard settles nothing
            val path = import.text
                .substringAfter("import")
                .substringBefore(';')
                .trim()
            if (path.substringAfterLast('.') != name) continue
            val pkg = path.substringBeforeLast('.', "")
            if (pkg.isNotEmpty()) return pkg
        }
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
            for (decl in symbolsIn(psi)) {
                action(decl)
            }
        }
    }

    private inline fun forEachType(project: Project, action: (JuxTypeDeclaration) -> Unit) =
        forEachType(project, GlobalSearchScope.allScope(project), action)
}
