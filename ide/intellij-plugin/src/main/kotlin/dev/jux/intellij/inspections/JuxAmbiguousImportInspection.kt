package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.editor.JuxImportSupport
import dev.jux.intellij.highlight.JuxKeywords
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.psi.JuxTypeParameter
import dev.jux.intellij.resolve.JuxImports
import dev.jux.intellij.resolve.JuxTypeEngine
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * E0303 mirrored IDE-side for the wildcard case (ERRATA E132, JLS 6.5.5.1):
 * a simple name that two wildcard imports bring for DIFFERENT types is
 * ambiguous where it is used -- in a type, a `new` or a static access -- and
 * nothing written says which one is meant.
 *
 * Importing both packages is not the error: an unused ambiguity, or a name
 * only one of them brings, is fine. A declaration of the file, a type of its
 * own package and a single-type import of the name all bind it more strongly
 * than a wildcard, so each of them settles it. Two spellings of one type (an
 * alias in a crate family's nested package, `rust.eframe.egui.Ui` for
 * `rust.eframe.Ui`) are one type, not an ambiguity.
 *
 * The quick-fixes import one candidate by name, which is what the compiler's
 * message recommends.
 *
 * The same code covers two single-type imports binding one simple name to
 * different types, reported on the later import ([checkConflictingImports]).
 */
class JuxAmbiguousImportInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        if (file.name.endsWith(".jux.d")) return null
        if (DumbService.isDumb(file.project)) return null
        val imports = JuxImportSupport.collectImports(file)
        val problems = ArrayList<ProblemDescriptor>()
        checkConflictingImports(file, imports, manager, isOnTheFly, problems)
        val wildcardPackages = imports.filter { it.wildcard }.map { it.path }.toSet()
        if (wildcardPackages.size < 2) return problems.toTypedArray()

        // What binds a simple name more strongly than a wildcard.
        val explicit = imports.filterNot { it.wildcard }.flatMapTo(HashSet()) { it.boundNames }
        val ownPackage = JuxAutoImport.effectivePackageOfFile(file)
        val declaredHere = JuxTypeIndex.typesIn(file).mapNotNullTo(HashSet()) { it.name }

        val verdicts = HashMap<String, List<String>>()
        fun ambiguity(name: String): List<String> = verdicts.getOrPut(name) {
            if (name in explicit || name in declaredHere) return@getOrPut emptyList()
            val all = JuxTypeIndex.typesNamed(file.project, name)
            if (all.any { JuxAutoImport.effectivePackageOf(it) == ownPackage }) return@getOrPut emptyList()
            // Canonical type -> the first FQN seen for it, as the compiler lists them.
            val byType = LinkedHashMap<JuxTypeDeclaration, String>()
            for (decl in all.sortedBy { JuxAutoImport.packageOf(it) }) {
                val pkg = JuxAutoImport.packageOf(decl)
                if (pkg !in wildcardPackages) continue
                val canonical = JuxTypeEngine.aliasTargetDeclaration(decl) ?: decl
                byType.putIfAbsent(canonical, "$pkg.$name")
            }
            if (byType.size < 2) emptyList() else byType.values.sorted()
        }

        PsiTreeUtil.processElements(file) { e ->
            val use = useSite(e)
            if (use != null) {
                val (name, anchor) = use
                val candidates = ambiguity(name)
                if (candidates.isNotEmpty() && !boundLocally(e, name)) {
                    val listed = if (candidates.size == 2) {
                        "both '${candidates[0]}' and '${candidates[1]}'"
                    } else {
                        "all of " + candidates.joinToString(", ") { "'$it'" }
                    }
                    problems.add(
                        manager.createProblemDescriptor(
                            anchor,
                            "Reference to '$name' is ambiguous: $listed match, each brought in by a wildcard " +
                                "import -- import the one you mean by name ('import ${candidates.last()};') or " +
                                "write its fully-qualified name (E0303)",
                            isOnTheFly,
                            candidates.map { ImportOneFix(it) }.toTypedArray<LocalQuickFix>(),
                            ProblemHighlightType.GENERIC_ERROR,
                        ),
                    )
                }
            }
            true
        }
        return problems.toTypedArray()
    }

    /**
     * Two single-type imports binding one simple name to different types
     * (`import alpha.Widget; import beta.Widget;`, or an alias clashing with
     * another import), reported on the later one. Two spellings of ONE type
     * -- a crate family's nested alias `rust.eframe.egui.Ui` beside
     * `rust.eframe.Ui` -- are one import (ERRATA E132). Only imports whose
     * targets both resolve are judged, so a missing stub never makes one.
     */
    private fun checkConflictingImports(
        file: JuxFile,
        imports: List<JuxImportSupport.ImportInfo>,
        manager: InspectionManager,
        isOnTheFly: Boolean,
        out: MutableList<ProblemDescriptor>,
    ) {
        val first = HashMap<String, Pair<String, JuxTypeDeclaration>>()
        for (import in imports) {
            if (import.wildcard) continue
            for ((bound, fqn) in import.targets) {
                val pkg = fqn.substringBeforeLast('.', "")
                val simple = fqn.substringAfterLast('.')
                if (pkg.isEmpty()) continue
                val decl = JuxTypeEngine.findTypeByFqn(file, pkg, simple) ?: continue
                val canonical = JuxTypeEngine.aliasTargetDeclaration(decl) ?: decl
                val previous = first[bound]
                if (previous == null) {
                    first[bound] = fqn to canonical
                } else if (previous.second != canonical) {
                    out.add(
                        manager.createProblemDescriptor(
                            import.element,
                            "Conflicting imports: the name '$bound' is imported from both '${previous.first}' and " +
                                "'$fqn' -- import only one of them, give one an 'as' alias " +
                                "('import $fqn as ${bound}2;'), or use a fully-qualified name (E0303)",
                            isOnTheFly,
                            LocalQuickFix.EMPTY_ARRAY,
                            ProblemHighlightType.GENERIC_ERROR,
                        ),
                    )
                }
            }
        }
    }

    /**
     * The simple name [e] uses as a type, with the leaf to underline: a bare
     * type reference (a declared type, a `new`, a cast, a pattern) or the head
     * of a static access (`Widget.make()`). Null for anything else, and for
     * anything inside an `import` or `package` statement.
     */
    private fun useSite(e: PsiElement): Pair<String, PsiElement>? {
        val name = when (e.elementType) {
            E.TYPE_REFERENCE -> JuxImports.bareTypeName(e)
            E.REFERENCE_EXPRESSION -> {
                val parent = e.parent
                if (parent?.elementType !== E.FIELD_ACCESS_EXPRESSION || parent.firstChild !== e) return null
                JuxImports.bareTypeName(e)
            }
            else -> null
        } ?: return null
        if (name in JuxKeywords.PRIMITIVES) return null
        if (PsiTreeUtil.findFirstParent(e) { it.elementType === E.IMPORT_STATEMENT || it.elementType === E.PACKAGE_STATEMENT } != null) {
            return null
        }
        val leaf = e.node.getChildren(null).firstOrNull { it.elementType === T.IDENTIFIER && it.text == name }?.psi
            ?: return null
        return name to leaf
    }

    /**
     * Whether [name] at [use] means something nearer than an import: a
     * generic parameter in scope, or (in an expression) a local, parameter or
     * field of that name (JLS 6.4.2).
     */
    private fun boundLocally(use: PsiElement, name: String): Boolean {
        if (JuxTypeEngine.resolveTypeName(use, name) is JuxTypeParameter) return true
        if (use.elementType === E.REFERENCE_EXPRESSION) {
            val target = JuxTypeEngine.resolveReferenceExpression(use)
            if (target != null && target !is JuxTypeDeclaration) return true
        }
        return false
    }

    /** Adds `import <fqn>;`, the single-type import that settles the ambiguity. */
    private class ImportOneFix(private val fqn: String) : LocalQuickFix {
        override fun getName(): String = "Import '$fqn'"

        override fun getFamilyName(): String = "Import the type by name"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val file = descriptor.psiElement?.containingFile as? JuxFile ?: return
            val document = PsiDocumentManager.getInstance(project).getDocument(file) ?: return
            val last = JuxImportSupport.collectImports(file).lastOrNull()?.element ?: return
            document.insertString(last.textRange.endOffset, "\nimport $fqn;")
            PsiDocumentManager.getInstance(project).commitDocument(document)
        }
    }
}
