package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.editor.JuxImportSupport
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxCompositeElement
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFieldDeclaration
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxLocalVariable
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxParameter
import dev.jux.intellij.psi.JuxPropertyDeclaration
import dev.jux.intellij.resolve.JuxReference

/**
 * Unused-symbol detection over the in-file resolver: a local variable,
 * parameter, or `private` non-static field with **zero references that
 * resolve to it** ([JuxReference.resolveLocally] — shadowing-correct by
 * construction, since each use resolves to the declaration that actually
 * shadows the rest).
 *
 * Conservative skips (no false positives by design):
 * - parameters of bodyless methods (interface/abstract — the signature is
 *   the contract) and of `@override` methods (fixed by the supertype);
 * - non-private fields (visible to other files; the LSP owns that analysis);
 * - anything named `_` (the conventional discard).
 */
class JuxUnusedLocalSymbolInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null

        // 1) Candidate declarations.
        val candidates = ArrayList<JuxNamedElement>()
        PsiTreeUtil.processElements(file) { e ->
            when (e) {
                is JuxLocalVariable -> candidates.add(e)
                is JuxParameter -> if (parameterCounts(e)) candidates.add(e)
                // Observable properties (a JuxFieldDeclaration subclass) have
                // their own §P inspections (W0971 never-observed/bound) and rich
                // usage via observers/bind, so don't double-flag them here as
                // "unused fields" (and never offer the destructive delete on one).
                is JuxPropertyDeclaration -> {}
                is JuxFieldDeclaration -> if (isPrivateInstanceField(e)) candidates.add(e)
            }
            true
        }
        if (candidates.isEmpty()) return null

        // 2) Usage census: resolve every in-file reference once and count.
        val usedDecls = HashSet<PsiElement>()
        // Names mentioned where the resolver is BLIND — interpolation holes
        // (`$"${count}"` is one token), switch-case patterns (raw identifier
        // leaves), and opaque runs (anonymous-class bodies, annotation args,
        // where-clauses). A candidate whose name appears there might be used,
        // so it must never be flagged (the quick-fix deletes code).
        val blindMentions = HashSet<String>()
        PsiTreeUtil.processElements(file) { e ->
            if (e is JuxCompositeElement) {
                val ref = e.references.firstOrNull() as? JuxReference
                ref?.resolveLocally()?.let(usedDecls::add)
            }
            when (e.elementType) {
                JuxTokenTypes.INTERP_STRING_LITERAL ->
                    blindMentions.addAll(JuxImportSupport.interpolatedNames(e.text, raw = false))
                JuxTokenTypes.INTERP_RAW_STRING_LITERAL ->
                    blindMentions.addAll(JuxImportSupport.interpolatedNames(e.text, raw = true))
                JuxTokenTypes.IDENTIFIER ->
                    if (e.parent?.elementType in BLIND_PARENTS) blindMentions.add(e.text)
                else -> {}
            }
            true
        }

        // 3) Report the unreferenced.
        val problems = ArrayList<ProblemDescriptor>()
        for (decl in candidates) {
            val name = decl.name ?: continue
            if (name == "_") continue
            if (decl in usedDecls) continue
            if (name in blindMentions) continue
            // `case Num(var n) | Neg(var n) -> n`: the alternatives bind one
            // name, and a use resolves to the first; any use counts for all.
            if (orPatternSiblings(decl).any { it in usedDecls }) continue
            val target = decl.nameIdentifier ?: continue
            val kind = when (decl) {
                is JuxLocalVariable -> "Variable"
                is JuxParameter -> "Parameter"
                else -> "Field"
            }
            // Deleting a pattern's binder would change the pattern's shape
            // (`Pt(a, b)` to `Pt(b)`), so a binder gets no removal fix.
            val isBinder = decl.parent?.elementType === E.PATTERN ||
                decl.parent?.elementType === E.DESTRUCTURING_DECLARATION ||
                dev.jux.intellij.psi.JuxLocals.isTypeTestBinder(decl)
            val fixes =
                if (isBinder) {
                    LocalQuickFix.EMPTY_ARRAY
                } else if (decl is JuxLocalVariable || decl is JuxFieldDeclaration) {
                    arrayOf<LocalQuickFix>(RemoveDeclarationFix(kind.lowercase()))
                } else if (decl is JuxParameter && RemoveUnusedParameterFix.callSites(decl) != null) {
                    // Only for a private method: every caller is in this file.
                    arrayOf<LocalQuickFix>(RemoveUnusedParameterFix())
                } else {
                    LocalQuickFix.EMPTY_ARRAY // removing a param changes a signature others call
                }
            problems.add(
                manager.createProblemDescriptor(
                    target,
                    "$kind '$name' is never used",
                    isOnTheFly,
                    fixes,
                    ProblemHighlightType.LIKE_UNUSED_SYMBOL,
                ),
            )
        }
        return problems.toTypedArray()
    }

    /**
     * The other binders of the same name in the same switch arm: in
     * `case Num(var n) | Neg(var n)` each `n` is the other's sibling.
     */
    private fun orPatternSiblings(decl: PsiElement): List<PsiElement> {
        if (decl.elementType !== E.LOCAL_VARIABLE) return emptyList()
        var arm: PsiElement? = decl.parent
        while (arm != null && arm.elementType === E.PATTERN) arm = arm.parent
        if (arm?.elementType !== E.SWITCH_CASE) return emptyList()
        val name = (decl as? JuxNamedElement)?.name ?: return emptyList()
        return dev.jux.intellij.psi.JuxLocals.bindersInScope(arm, null)
            .filter { it !== decl && (it as? JuxNamedElement)?.name == name }
    }

    /**
     * Parameters only count when the enclosing method has a body and is not
     * `@override` (annotations are case-insensitive in Jux).
     */
    private fun parameterCounts(param: JuxParameter): Boolean {
        val list = param.parent ?: return false
        val method = list.parent ?: return false
        if (method.elementType !== E.METHOD_DECLARATION &&
            method.elementType !== E.CONSTRUCTOR_DECLARATION
        ) return false
        if (method.node.findChildByType(E.CODE_BLOCK) == null) return false // bodyless
        // `@override` parameters are dictated by the supertype.
        var c: PsiElement? = method.firstChild
        while (c != null) {
            if (c.elementType === E.ANNOTATION &&
                c.text.removePrefix("@").substringBefore('(').trim()
                    .equals("override", ignoreCase = true)
            ) return false
            c = c.nextSibling
        }
        return true
    }

    /** `private` and non-`static` — anything wider is visible cross-file. */
    private fun isPrivateInstanceField(field: JuxFieldDeclaration): Boolean {
        val mods = field.node.findChildByType(E.MODIFIER_LIST)?.psi?.text ?: return false
        val padded = " $mods "
        return padded.contains(" private ") && !padded.contains(" static ")
    }

    private companion object {
        /** Parents whose identifier leaves carry no resolvable reference. */
        val BLIND_PARENTS = setOf(E.PATTERN, E.NEW_EXPRESSION, E.ANNOTATION, E.WHERE_CLAUSE)
    }

    /** Deletes the whole declaration (offered for locals and fields only). */
    private class RemoveDeclarationFix(private val kind: String) : LocalQuickFix {
        override fun getFamilyName(): String = "Remove unused $kind"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            // The descriptor points at the name identifier; delete its declaration.
            val decl = descriptor.psiElement?.parent ?: return
            val next = decl.nextSibling
            decl.delete()
            if (next is com.intellij.psi.PsiWhiteSpace && next.isValid && next.text.startsWith("\n")) {
                next.delete()
            }
        }
    }
}
