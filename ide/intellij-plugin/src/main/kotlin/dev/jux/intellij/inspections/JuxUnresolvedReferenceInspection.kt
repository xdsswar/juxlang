package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.util.CachedValueProvider
import com.intellij.psi.util.CachedValuesManager
import com.intellij.psi.util.PsiModificationTracker
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.editor.JuxImportSupport
import dev.jux.intellij.highlight.JuxKeywords
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.resolve.JuxReference
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * Live "Cannot resolve symbol / type" detection — the IDE feedback that lights
 * up the moment a local / parameter / field / method / function / **type** is
 * renamed away and a usage below is left dangling, with no compiler round-trip.
 *
 * Two positions are covered: bare value references (`REFERENCE_EXPRESSION`) and
 * type references (`TYPE_REFERENCE`), each with its own gate ([shouldFlagValue] /
 * [shouldFlagType]). Member accesses (`obj.x`) stay with the language server.
 *
 * The deliberate non-goal is matching `juxc` exactly; the deliberate goal is
 * **zero false positives** on valid code (the reason [JuxReference] itself stays
 * soft). A reference is flagged only when EVERY way the name could be
 * legitimately bound has been ruled out:
 *
 *  - its first letter is lowercase — a bare *Capitalized* name may be a
 *    type-as-value or an unimported std singleton, which the LSP owns;
 *  - it is not a keyword / primitive / literal constant nor a built-in global
 *    ([JuxKeywords] + [BUILTIN_GLOBALS]);
 *  - it is **not introduced as a binding anywhere in the file** — every binding
 *    occurrence (declaration / parameter / `for` var / `catch` var / lambda
 *    param / destructuring / pattern) is an identifier sitting in a *non-
 *    reference* position, so [definedNames] collects them all without having to
 *    model each construct. This is what makes loop/lambda/catch vars safe even
 *    though the per-file resolver doesn't scope them;
 *  - it is not bound by an `import`, and the file has **no** wildcard import (a
 *    `*` could supply any name — [JuxImportSupport]);
 *  - it is not declared **anywhere in the project** as a type / method / field /
 *    enum constant ([JuxTypeIndex.forEachSymbol]) — the cross-file / same-
 *    package safety net;
 *  - it does not sit under a node the resolver is blind to ([BLIND_ANCESTORS]).
 *
 * The remaining names are genuinely unknown: a typo, or a usage orphaned by a
 * rename. Quick-fix: change the usage to the nearest in-scope declaration.
 */
class JuxUnresolvedReferenceInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null

        // A wildcard import could bind any of the flagged names — stand down for
        // the whole file rather than risk a false positive. Explicitly bound
        // import names are always treated as resolved.
        val imports = JuxImportSupport.collectImports(file)
        if (imports.any { it.alwaysKeep }) return null
        val importedNames = HashSet<String>()
        imports.forEach { importedNames.addAll(it.boundNames) }

        // Every name introduced as a binding in THIS file (covers all binding
        // constructs at once — see class doc) plus every declared symbol name in
        // the project. A name in either set is never "unknown".
        val definedNames = collectDefinedNames(file)
        val projectNames = projectDeclaredNames(file.project)

        val problems = ArrayList<ProblemDescriptor>()
        PsiTreeUtil.processElements(file) { e ->
            val isValue = e.elementType === E.REFERENCE_EXPRESSION
            val isType = e.elementType === E.TYPE_REFERENCE
            if (isValue || isType) {
                val ref = e.references.firstOrNull() as? JuxReference
                if (ref != null) {
                    val name = ref.value
                    val target = e.findElementAt(ref.rangeInElement.startOffset)
                    val flag =
                        if (isType) shouldFlagType(e, name, definedNames, importedNames, projectNames)
                        else shouldFlagValue(e, name, definedNames, importedNames, projectNames)
                    if (target != null && flag) {
                        val noun = if (isType) "type" else "symbol"
                        problems.add(
                            manager.createProblemDescriptor(
                                target,
                                "Cannot resolve $noun '$name'",
                                isOnTheFly,
                                fixesFor(e, name, isType),
                                ProblemHighlightType.LIKE_UNKNOWN_SYMBOL,
                            ),
                        )
                    }
                }
            }
            true
        }
        return problems.toTypedArray()
    }

    /**
     * Value gate (bare `REFERENCE_EXPRESSION`): the escape hatches from the class
     * doc. Restricted to lowercase names — a capitalized bare reference may be a
     * type-as-value / std singleton, which the LSP owns.
     */
    private fun shouldFlagValue(
        element: PsiElement,
        name: String,
        definedNames: Set<String>,
        importedNames: Set<String>,
        projectNames: Set<String>,
    ): Boolean {
        if (name.isEmpty() || name == "_") return false
        if (!name[0].isLowerCase()) return false
        if (name in JuxKeywords.KEYWORDS || name in JuxKeywords.PRIMITIVES ||
            name in JuxKeywords.CONSTANTS || name in BUILTIN_GLOBALS
        ) return false
        if (name in definedNames || name in importedNames || name in projectNames) return false
        return !isBlind(element)
    }

    /**
     * Type gate (`TYPE_REFERENCE`): mirrors the value gate but keeps capitalized
     * names (types usually are) and adds the always-in-scope [STD_PRELUDE_TYPES]
     * — the `jux.std` `java.lang`-style prelude (`Map`, `List`, `Throwable`, …)
     * the compiler prepends to every unit. Type parameters and in-file types are
     * already covered by [definedNames]; `rust.std` types (`Vec`, `Box`, …) need
     * an `import`, so flagging them unqualified matches `juxc`'s own E0301.
     *
     * Qualified references (`a.b.C`) are skipped — package-path resolution is the
     * language server's job, not the per-file resolver's.
     */
    private fun shouldFlagType(
        element: PsiElement,
        name: String,
        definedNames: Set<String>,
        importedNames: Set<String>,
        projectNames: Set<String>,
    ): Boolean {
        if (name.isEmpty() || name == "_") return false
        if (element.text.substringBefore('<').contains('.')) return false // qualified → LSP
        if (name in JuxKeywords.KEYWORDS || name in JuxKeywords.PRIMITIVES ||
            name == "observer" || name in STD_PRELUDE_TYPES
        ) return false
        if (name in definedNames || name in importedNames || name in projectNames) return false
        return !isBlind(element)
    }

    /**
     * Names bound anywhere in [file]: the text of every identifier leaf that is
     * NOT itself a use (its parent is not a reference/type-reference node). This
     * single pass captures declaration names, parameters, `for`/`catch`/lambda
     * variables, destructuring and pattern bindings alike.
     */
    private fun collectDefinedNames(file: PsiFile): Set<String> {
        val names = HashSet<String>()
        PsiTreeUtil.processElements(file) { e ->
            if (e.elementType === JuxTokenTypes.IDENTIFIER && e.parent?.elementType !in USE_PARENTS) {
                names.add(e.text)
            }
            true
        }
        return names
    }

    /**
     * True when the reference lives under a node the in-file resolver can't see
     * into — switch-case patterns, annotation arguments, `where` clauses — so a
     * non-resolving name there is never a reliable error.
     */
    private fun isBlind(element: PsiElement): Boolean {
        var p: PsiElement? = element.parent
        while (p != null && p !is JuxFile) {
            if (p.elementType in BLIND_ANCESTORS) return true
            p = p.parent
        }
        return false
    }

    /**
     * Offer "change to the nearest declaration" when a known name is a close
     * spelling match (edit distance ≤ 2) — the orphaned-by-rename / typo fix.
     * Candidate pool depends on position: in-scope declarations for a value,
     * visible type names for a type.
     */
    private fun fixesFor(element: PsiElement, name: String, isType: Boolean): Array<LocalQuickFix> {
        val suggestion =
            (if (isType) nearestTypeName(element, name) else nearestVisibleName(element, name))
                ?: return LocalQuickFix.EMPTY_ARRAY
        return arrayOf(RenameReferenceFix(suggestion))
    }

    /**
     * The type name closest to [name] by edit distance (≤ 2): project-wide type
     * declarations plus the [STD_PRELUDE_TYPES]. Null when nothing is close.
     */
    private fun nearestTypeName(element: PsiElement, name: String): String? {
        val candidates = LinkedHashSet<String>()
        candidates.addAll(JuxTypeIndex.allTypeNames(element.project))
        candidates.addAll(STD_PRELUDE_TYPES)
        candidates.remove(name)
        return candidates
            .map { it to levenshtein(name, it) }
            .filter { it.second in 1..2 }
            .minByOrNull { it.second }
            ?.first
    }

    /**
     * The visible declaration name closest to [name] by edit distance (≤ 2),
     * gathered from the enclosing scopes (block locals, method params, class
     * members, file declarations). Null when nothing is close enough — better no
     * fix than a misleading one.
     */
    private fun nearestVisibleName(element: PsiElement, name: String): String? {
        val candidates = LinkedHashSet<String>()
        var scope: PsiElement? = element.parent
        while (scope != null) {
            when (scope.elementType) {
                E.CODE_BLOCK ->
                    for (child in scope.children) {
                        if (child is JuxNamedElement && child.elementType === E.LOCAL_VARIABLE) {
                            child.name?.let(candidates::add)
                        }
                    }
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                    scope.children.firstOrNull { it.elementType === E.PARAMETER_LIST }
                        ?.children?.forEach { p ->
                            if (p is JuxNamedElement) p.name?.let(candidates::add)
                        }
                E.CLASS_BODY ->
                    for (m in scope.children) if (m is JuxNamedElement) m.name?.let(candidates::add)
            }
            if (scope is JuxFile) {
                for (d in scope.children) if (d is JuxNamedElement) d.name?.let(candidates::add)
            }
            scope = scope.parent
        }
        candidates.remove(name)
        return candidates
            .map { it to levenshtein(name, it) }
            .filter { it.second in 1..2 }
            .minByOrNull { it.second }
            ?.first
    }

    /** Project-wide declared symbol names (types/methods/fields/enum constants), cached. */
    private fun projectDeclaredNames(project: Project): Set<String> =
        CachedValuesManager.getManager(project).getCachedValue(project) {
            val names = HashSet<String>()
            JuxTypeIndex.forEachSymbol(project, GlobalSearchScope.allScope(project)) { d ->
                d.name?.let(names::add)
            }
            CachedValueProvider.Result.create(names, PsiModificationTracker.MODIFICATION_COUNT)
        }

    /** Classic edit distance, capped implicitly by the ≤ 2 filter at the call site. */
    private fun levenshtein(a: String, b: String): Int {
        val prev = IntArray(b.length + 1) { it }
        val curr = IntArray(b.length + 1)
        for (i in 1..a.length) {
            curr[0] = i
            for (j in 1..b.length) {
                val cost = if (a[i - 1] == b[j - 1]) 0 else 1
                curr[j] = minOf(curr[j - 1] + 1, prev[j] + 1, prev[j - 1] + cost)
            }
            System.arraycopy(curr, 0, prev, 0, curr.size)
        }
        return prev[b.length]
    }

    private companion object {
        /**
         * Identifier parents that mark a *use* rather than a binding — every
         * other identifier position is treated as introducing a name (see
         * [collectDefinedNames]).
         */
        val USE_PARENTS = setOf(
            E.REFERENCE_EXPRESSION,
            E.TYPE_REFERENCE,
            E.FIELD_ACCESS_EXPRESSION,
            E.METHOD_REF_EXPRESSION,
        )

        /**
         * The `jux.std` prelude types the compiler prepends to every unit, so
         * they resolve unqualified `java.lang`-style (see `juxc-driver`'s
         * `stdlib` loader / `stdlib_embedded`). Mirrors the public type
         * declarations there — keep in sync if that surface changes. Deliberately
         * excludes `rust.std`-only types (`Vec`, `Box`, `Rc`, …): those require an
         * `import`, so an unqualified use of them is a real error.
         */
        val STD_PRELUDE_TYPES = setOf(
            "String", "Object", "Self",
            "ArrayList", "Collection", "HashMap", "HashSet", "Deque",
            "Iterable", "Iterator", "List", "Map", "Set",
            "MemoryOrder", "AtomicInt", "AtomicLong", "Worker",
            "Option", "Result", "Clock", "Instant", "File", "Path", "Console",
            "Throwable", "Error", "Exception", "RuntimeException",
            "ArithmeticException", "ClassCastException", "FileNotFoundException",
            "IllegalArgumentException", "IllegalStateException",
            "IndexOutOfBoundsException", "IOException", "NoSuchElementException",
            "NullPointerException", "TimeoutException", "CancellationException",
            "UnsupportedOperationException",
        )

        /**
         * Unqualified names that resolve outside the file with no `import` — the
         * Phase-1 prelude / intrinsics. Over-inclusion is the safe direction (it
         * only suppresses a diagnostic), so this errs broad.
         */
        val BUILTIN_GLOBALS = setOf(
            "print", "println", "eprint", "eprintln", "format",
            "panic", "todo", "unreachable", "assert", "assert_eq", "debug_assert",
            "this", "super", "self", "it",
        )

        /** Ancestor node kinds whose identifier leaves the resolver can't see into. */
        val BLIND_ANCESTORS = setOf(E.PATTERN, E.ANNOTATION, E.WHERE_CLAUSE)
    }

    /** Rewrites the dangling name leaf to a close, in-scope declaration name. */
    private class RenameReferenceFix(private val suggestion: String) : LocalQuickFix {
        override fun getName(): String = "Change to '$suggestion'"

        override fun getFamilyName(): String = "Change to nearest declaration"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val leaf = descriptor.psiElement ?: return
            leaf.replace(JuxElementFactory.createIdentifier(project, suggestion))
        }
    }
}
