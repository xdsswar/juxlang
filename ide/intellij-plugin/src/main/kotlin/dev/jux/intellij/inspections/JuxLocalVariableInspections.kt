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
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxLocalVariable

/**
 * The plain local variables of [file]: the ones a declaration statement
 * introduces directly in a block (`int n = 0;`, `var s = f();`), not a loop
 * variable, a caught exception, a pattern binder or a destructuring binder.
 * A `ref` local is left out: it shares its cell with another name, so a
 * write through it is a write the other name sees.
 */
internal fun plainLocals(file: PsiFile): List<JuxLocalVariable> =
    PsiTreeUtil.findChildrenOfType(file, JuxLocalVariable::class.java).filter {
        it.parent?.elementType === E.CODE_BLOCK &&
            it.node.findChildByType(T.IDENTIFIER) != null &&
            it.node.findChildByType(T.REF_KW) == null
    }

/**
 * "Local variable may be 'final'", Java's `LocalCanBeFinal`: a local that is
 * set once, where it is declared, and never again.
 *
 * Off by default, as in Java: it is a style many code bases do not follow.
 * The fix writes `final` in front of the declaration.
 */
class JuxLocalMayBeFinalInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val locals = plainLocals(file).filter {
            JuxAccess.initializerOf(it) != null && "final" !in JuxAccess.modifiers(it) && "const" !in JuxAccess.modifiers(it)
        }
        if (locals.isEmpty()) return null
        val census = JuxAccess.census(file)
        return locals.mapNotNull { local ->
            val name = local.name ?: return@mapNotNull null
            if (name == "_" || name in census.unresolved) return@mapNotNull null
            val written = census.uses[local].orEmpty().any { JuxAccess.writes(JuxAccess.kindOf(it)) }
            if (written) return@mapNotNull null
            val target = local.nameIdentifier ?: return@mapNotNull null
            manager.createProblemDescriptor(
                target,
                "Variable '$name' can be 'final'",
                isOnTheFly,
                arrayOf<LocalQuickFix>(AddFinalFix()),
                ProblemHighlightType.GENERIC_ERROR_OR_WARNING,
            )
        }.toTypedArray()
    }

    override fun isEnabledByDefault(): Boolean = false

    private class AddFinalFix : LocalQuickFix {
        override fun getFamilyName(): String = "Make 'final'"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val local = descriptor.psiElement?.parent ?: return
            val start = local.textRange.startOffset
            JuxCodeFacts.edit(project, local.containingFile, start, start, "final ")
        }
    }
}

/**
 * "Explicit type can be replaced with 'var'", Java's `LocalCanBeVar` with
 * the conservative rule: only where the initializer spells the same type,
 * `Point p = new Point(1, 2);`. `Vec<int> v = new Vec<>();` stays: with `var`
 * the diamond would have nothing to infer from.
 *
 * Off by default, like Java's.
 */
class JuxTypeCanBeVarInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        return plainLocals(file).mapNotNull { local ->
            val typeRef = local.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return@mapNotNull null
            val init = JuxAccess.initializerOf(local)?.takeIf { it.elementType === E.NEW_EXPRESSION } ?: return@mapNotNull null
            val written = compact(typeRef.text)
            val constructed = compact(init.node.findChildByType(E.TYPE_REFERENCE)?.text ?: return@mapNotNull null)
            if (written != constructed || written.contains("<>") || written.endsWith("?")) return@mapNotNull null
            // An array allocation `new int[n]` names the element type only.
            if (init.text.contains('[')) return@mapNotNull null
            manager.createProblemDescriptor(
                typeRef,
                "Explicit type can be replaced with 'var'",
                isOnTheFly,
                arrayOf<LocalQuickFix>(UseVarFix()),
                ProblemHighlightType.GENERIC_ERROR_OR_WARNING,
            )
        }.toTypedArray()
    }

    override fun isEnabledByDefault(): Boolean = false

    private fun compact(text: String) = text.filterNot { it.isWhitespace() }

    private class UseVarFix : LocalQuickFix {
        override fun getFamilyName(): String = "Replace with 'var'"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val typeRef = descriptor.psiElement ?: return
            JuxCodeFacts.edit(project, typeRef.containingFile, typeRef.textRange.startOffset, typeRef.textRange.endOffset, "var")
        }
    }
}

/**
 * "Variable is assigned but never read", the part of Java's
 * `UnusedAssignment` that needs no flow graph: a local that is given values
 * (at its declaration or later) and never read anywhere. The unused-symbol
 * inspection already covers a local with no uses at all; this one reports
 * the local whose every use is a plain `x = e`.
 *
 * No fix: the values assigned may come from calls whose effects matter.
 */
class JuxAssignedNeverReadInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val locals = plainLocals(file)
        if (locals.isEmpty()) return null
        val census = JuxAccess.census(file)
        val blind = JuxAccess.blindMentions(file)
        return locals.mapNotNull { local ->
            val name = local.name ?: return@mapNotNull null
            if (name == "_" || name in blind || name in census.unresolved) return@mapNotNull null
            val uses = census.uses[local].orEmpty()
            if (uses.isEmpty()) return@mapNotNull null // the unused-symbol inspection's case
            if (uses.any { JuxAccess.reads(JuxAccess.kindOf(it)) }) return@mapNotNull null
            val target = local.nameIdentifier ?: return@mapNotNull null
            manager.createProblemDescriptor(
                target,
                "Variable '$name' is assigned but never read",
                isOnTheFly,
                LocalQuickFix.EMPTY_ARRAY,
                ProblemHighlightType.LIKE_UNUSED_SYMBOL,
            )
        }.toTypedArray()
    }
}

/** The declaration a plain name [ref] names: a local, parameter or field. */
internal fun storageOf(ref: PsiElement): PsiElement? {
    val target = runCatching { dev.jux.intellij.resolve.JuxTypeEngine.resolveReferenceExpression(ref) }.getOrNull() ?: return null
    return target.takeIf { it.elementType === E.LOCAL_VARIABLE || it.elementType === E.PARAMETER || it.elementType === E.FIELD_DECLARATION }
}
