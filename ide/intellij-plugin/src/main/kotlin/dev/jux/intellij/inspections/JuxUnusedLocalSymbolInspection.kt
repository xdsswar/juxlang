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
import dev.jux.intellij.psi.JuxCompositeElement
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFieldDeclaration
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxLocalVariable
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxParameter
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
                is JuxFieldDeclaration -> if (isPrivateInstanceField(e)) candidates.add(e)
            }
            true
        }
        if (candidates.isEmpty()) return null

        // 2) Usage census: resolve every in-file reference once and count.
        val usedDecls = HashSet<PsiElement>()
        PsiTreeUtil.processElements(file) { e ->
            if (e is JuxCompositeElement) {
                val ref = e.references.firstOrNull() as? JuxReference
                ref?.resolveLocally()?.let(usedDecls::add)
            }
            true
        }

        // 3) Report the unreferenced.
        val problems = ArrayList<ProblemDescriptor>()
        for (decl in candidates) {
            val name = decl.name ?: continue
            if (name == "_") continue
            if (decl in usedDecls) continue
            val target = decl.nameIdentifier ?: continue
            val kind = when (decl) {
                is JuxLocalVariable -> "Variable"
                is JuxParameter -> "Parameter"
                else -> "Field"
            }
            val fixes =
                if (decl is JuxLocalVariable || decl is JuxFieldDeclaration) {
                    arrayOf<LocalQuickFix>(RemoveDeclarationFix(kind.lowercase()))
                } else {
                    LocalQuickFix.EMPTY_ARRAY // removing a param changes the signature
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
