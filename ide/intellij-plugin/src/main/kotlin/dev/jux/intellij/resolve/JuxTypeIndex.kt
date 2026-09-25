package dev.jux.intellij.resolve

import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Key
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.GlobalSearchScopesCore
import com.intellij.psi.search.FileTypeIndex
import com.intellij.psi.util.CachedValue
import com.intellij.psi.util.CachedValueProvider
import com.intellij.psi.util.CachedValuesManager
import com.intellij.psi.util.PsiModificationTracker
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.editor.JuxImportSupport
import dev.jux.intellij.highlight.JuxKeywords
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
        // The bundled `jux.std` sources are left out: their MEMBER names
        // (`value`, `count`, `map`) must not make a bare name look declared.
        // The std type names are in scope anyway, as the generated built-in
        // names the callers check first.
        val all = GlobalSearchScope.allScope(project)
        val scope = JuxBundledStd.root()
            ?.let { all.intersectWith(GlobalSearchScope.notScope(GlobalSearchScopesCore.directoryScope(project, it, true))) }
            ?: all
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
            // A populated index that knows no such type is the answer: `int`
            // and `String` are asked about constantly, and falling through to
            // the project-wide map rebuilt it on every keystroke.
            if (JuxDeclarationIndex.hasData(project)) return null
        }
        // No index yet: the project-wide map, which is still cheaper than the
        // scan this replaced.
        return typesByName(project)[name]?.firstOrNull()
    }

    /**
     * The type named [name] **as seen from [context]**: the §M.16 resolution
     * ladder, as far as the IDE can walk it.
     *
     * This is the overload nearly every caller wants. A single-segment name is
     * resolved in the compilation UNIT that wrote it and nowhere else
     * (`JUX-MISSING-DEFS-ADDENDUM.md` §M.16, ERRATA E96), by this ladder:
     *
     *  1. a generic parameter in scope (the caller's rung: see
     *     [JuxTypeEngine.resolveTypeName], which tries type parameters before
     *     it gets here);
     *  2. a declaration the unit itself makes, or one an `import` in the unit
     *     binds, or one the unit's own package makes;
     *  3. a nested type of the enclosing type (§M.9);
     *  4. the implicit prelude: `jux.std.*` and `rust.std`.
     *
     * A name that reaches none of those rungs is unresolved, and the honest
     * `null` matters: the inspections built on this layer stay silent on a name
     * they cannot resolve, so silence is what a name the unit never named has
     * to produce. Returning "some type of that name, from somewhere" instead is
     * what made the plugin paint red on correct code. `examples/` alone has
     * `Tagged` as an interface in one file and a class in two others, and
     * `examples/stdlib_name_collisions.jux` legally declares its own
     * `interface Iterable { String describe(); }` -- which the project-wide
     * lookup handed to `class Span implements Iterable<int>` in
     * `examples/iterable_combinators.jux`, an unrelated file, so E0429 demanded
     * `describe()` from a class that owes nothing of the kind.
     *
     * Two rules from §M.16 shape the walk below, and both are about the same
     * thing, that resolution is a question the ASKING unit gets to answer:
     *
     *  - **§M.16.1, the library realm.** `jux.std.*` and the generated `rust.*`
     *    stubs form a realm that never binds a bare name to a user
     *    declaration. It is what lets a program declare `class T`,
     *    `class String`, `class Vec`, `class Exception` or `interface Iterable`
     *    at all: inside the user's unit the name means the user's type, and
     *    inside `jux.std.collections.Iterable` the name `Iterator` still means
     *    `jux.std.collections.Iterator`. The two never meet.
     *  - **A prelude name is shadowed by the unit, not by the workspace.** The
     *    IDE holds every file of every program in one project -- the corpus
     *    tests put 300-odd independent single-file programs in one source root,
     *    and a source root full of samples is a shape real users have too -- so
     *    a sibling in the DEFAULT package is not evidence of the same unit the
     *    way a sibling in a NAMED package is. Over a name the prelude also
     *    spells, an unnamed-package sibling therefore does not shadow the
     *    prelude. This is the one place the IDE is deliberately narrower than
     *    the compiler's rung 2, and it is narrower in the safe direction: the
     *    cost is a missing diagnostic on a program that redeclares a standard
     *    library name in the default package and uses it from a second file,
     *    and the alternative cost is red on code that compiles.
     */
    fun findType(context: PsiElement, name: String): JuxTypeDeclaration? {
        val file = context.containingFile
            ?: return findType(context.project, name)

        // Rung 2, first half: the unit itself. A name declared here means here.
        for (decl in typesIn(file)) {
            if (decl.name == name) return decl
        }

        // §M.16.1: a unit of the library realm resolves against the realm only,
        // so a program's own `class T` is invisible from inside `jux.std`.
        val hereIsLibrary = isLibraryRealmPackage(JuxAutoImport.packageOfFile(file))
        val candidates = candidateTypes(context.project, name)
            .let { all -> if (hereIsLibrary) all.filter { isLibraryRealmPackage(JuxAutoImport.packageOf(it)) } else all }
        if (candidates.isEmpty()) return null

        // Rung 2, second half: an explicit import decides between same-named
        // types. Two `Animal`s in one project is not a mistake, and the file
        // already says which one it means -- reading `import poll.lib.Animal;`
        // is the difference between a correct answer and a coin toss.
        val importedPackage = importedPackageFor(file, name)
        if (importedPackage != null) {
            candidates.firstOrNull { JuxAutoImport.packageOf(it) == importedPackage }
                ?.let { return it }
        }

        // Rung 2, third half: a sibling in the same NAMED package needs no
        // import, so it wins next. The default package is deliberately left
        // out here and handled at the bottom: see the KDoc above for why a
        // package-less sibling is not evidence of the same unit.
        val ownPackage = JuxAutoImport.packageOfFile(file)
        if (ownPackage.isNotEmpty()) {
            candidates.firstOrNull { JuxAutoImport.packageOf(it) == ownPackage }
                ?.let { return it }
        }

        // Rung 4: the prelude. Reached when the unit neither declared nor
        // imported the name and its own named package does not have it.
        candidates.firstOrNull { isLibraryRealmPackage(JuxAutoImport.packageOf(it)) }
            ?.let { return it }

        // Still a prelude NAME, with no prelude source indexed to point at:
        // the bundled `jux.std` tree is a plugin resource and the `rust.*`
        // stubs depend on what the machine has built, so either can be absent
        // while the name still means the prelude's type. The answer is then
        // "unresolved", never a stranger's class of the same name -- which is
        // exactly the `Iterable` mix-up this method's KDoc describes.
        if (name in JuxKeywords.BUILTINS) return null

        // Last resort: a same-named type elsewhere in the workspace, default
        // package included. A plain user name that only one other file
        // declares is the ordinary cross-file case, and resolving it is the
        // whole point of a project-wide index.
        candidates.firstOrNull { JuxAutoImport.packageOf(it) == ownPackage }
            ?.let { return it }
        return candidates.first()
    }

    /**
     * Whether [pkg] is a **library realm** package (§M.16.1): the embedded
     * standard library (`jux.std` and below, plus the `jux.meta` annotation
     * surface) or a generated foreign-crate stub package (`rust.*`).
     *
     * Named exactly, not by prefix guesswork, and for the same reason the
     * compiler's `is_library_realm_package` is: a user's own `package jux;` is
     * legal and is NOT a library package (§M.16.3), and neither is
     * `juxtapose.core`.
     */
    fun isLibraryRealmPackage(pkg: String): Boolean =
        pkg == "jux.std" || pkg.startsWith("jux.std.") ||
            pkg == "jux.meta" ||
            pkg == "rust" || pkg.startsWith("rust.")

    /** Every type named [name] in the project and its libraries. */
    fun typesNamed(project: Project, name: String): List<JuxTypeDeclaration> = candidateTypes(project, name)

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
            // Trusted even when empty; see findType(project, name).
            return out
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
     * Visits the types in [scope] whose name passes [nameMatches], reading
     * only the files the name index says declare such a name.
     *
     * This is what completion calls on every popup: [forEachType] would touch
     * every Jux file in the project and its libraries, while the index narrows
     * the walk to the handful of files declaring a name that matches what has
     * been typed. A fixture whose index holds nothing yet falls back to the
     * full walk, filtered the same way.
     */
    inline fun forEachTypeMatching(
        project: Project,
        scope: GlobalSearchScope,
        crossinline nameMatches: (String) -> Boolean,
        action: (JuxTypeDeclaration) -> Unit,
    ) {
        if (DumbService.isDumb(project)) return
        if (!JuxDeclarationIndex.hasData(project)) {
            forEachType(project, scope) { decl -> if (decl.name?.let(nameMatches) == true) action(decl) }
            return
        }
        val names = ArrayList<String>()
        com.intellij.util.indexing.FileBasedIndex.getInstance().processAllKeys(
            JuxDeclarationIndex.NAME,
            { key -> if (nameMatches(key)) names.add(key); true },
            scope,
            null,
        )
        val manager = PsiManager.getInstance(project)
        val seen = HashSet<JuxTypeDeclaration>()
        for (name in names) {
            com.intellij.openapi.progress.ProgressManager.checkCanceled()
            for (vf in JuxDeclarationIndex.containingFiles(name, project, scope)) {
                val psi = manager.findFile(vf) ?: continue
                for (decl in typesIn(psi)) {
                    if (decl.name == name && seen.add(decl)) action(decl)
                }
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
