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
import dev.jux.intellij.psi.JuxFieldDeclaration
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxPropertyDeclaration

/**
 * "Field may be 'final'", Java's `FieldMayBeFinal`: a `private` instance field
 * of a class that is only ever set while the object is being built.
 *
 * Reported when either
 * - the field has an initializer and nothing assigns it afterwards, or
 * - it has none, the class has constructors, and each constructor that does
 *   not hand over to another with `this(...)` assigns it exactly once, as a
 *   plain `=` statement directly in its body (not in a branch, loop or
 *   lambda); nothing else assigns it.
 *
 * `private` keeps the question inside this file (a private member is
 * top-level scoped, so every use is here). Properties, `static`, `weak` and
 * `volatile` fields and structs are left out. A field whose name the
 * resolver could not place somewhere in the file is left out too: a use
 * nobody counted might be a write.
 */
class JuxFieldMayBeFinalInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val fields = PsiTreeUtil.findChildrenOfType(file, JuxFieldDeclaration::class.java)
            .filter { it !is JuxPropertyDeclaration && isCandidate(it) }
        if (fields.isEmpty()) return null
        val census = JuxAccess.census(file)
        val problems = ArrayList<ProblemDescriptor>()
        for (field in fields) {
            val name = field.name ?: continue
            if (name in census.unresolved) continue
            val uses = census.uses[field].orEmpty()
            if (!onlySetDuringConstruction(field, uses)) continue
            val target = field.nameIdentifier ?: continue
            problems.add(
                manager.createProblemDescriptor(
                    target,
                    "Field '$name' may be 'final'",
                    isOnTheFly,
                    arrayOf<LocalQuickFix>(MakeFinalFix()),
                    ProblemHighlightType.GENERIC_ERROR_OR_WARNING,
                ),
            )
        }
        return problems.toTypedArray()
    }

    private fun isCandidate(field: JuxFieldDeclaration): Boolean {
        val mods = JuxAccess.modifiers(field)
        if ("private" !in mods) return false
        if (mods.any { it in EXCLUDED }) return false
        val owner = field.parent?.parent ?: return false
        return owner.elementType === E.CLASS_DECLARATION && field.parent?.elementType === E.CLASS_BODY
    }

    private fun onlySetDuringConstruction(field: JuxFieldDeclaration, uses: List<PsiElement>): Boolean {
        val writes = uses.filter { JuxAccess.writes(JuxAccess.kindOf(it)) }
        if (JuxAccess.initializerOf(field) != null) return writes.isEmpty()

        val body = field.parent ?: return false
        val constructors = body.children.filter { it.elementType === E.CONSTRUCTOR_DECLARATION }
        if (constructors.isEmpty()) return false
        // Every write: a plain `=`, a statement directly in some constructor's body.
        val byConstructor = HashMap<PsiElement, Int>()
        for (w in writes) {
            if (JuxAccess.kindOf(w) != JuxAccess.Kind.WRITE) return false
            val ctor = directConstructorStatementOwner(w) ?: return false
            if (ctor.parent !== body) return false
            byConstructor[ctor] = (byConstructor[ctor] ?: 0) + 1
        }
        for (ctor in constructors) {
            val count = byConstructor[ctor] ?: 0
            if (delegatesToThis(ctor)) {
                if (count != 0) return false
            } else if (count != 1) {
                return false
            }
        }
        return true
    }

    /**
     * The constructor whose body holds the statement `x = e;` that [write] is
     * the left side of, directly (not nested in a branch, loop or lambda).
     */
    private fun directConstructorStatementOwner(write: PsiElement): PsiElement? {
        var e: PsiElement = write
        while (e.parent?.elementType === E.PARENTHESIZED_EXPRESSION) e = e.parent
        val assignment = e.parent?.takeIf { it.elementType === E.ASSIGNMENT_EXPRESSION } ?: return null
        val statement = assignment.parent?.takeIf { it.elementType === E.EXPRESSION_STATEMENT } ?: return null
        val block = statement.parent?.takeIf { it.elementType === E.CODE_BLOCK } ?: return null
        return block.parent?.takeIf { it.elementType === E.CONSTRUCTOR_DECLARATION }
    }

    /** `this(...)` as the constructor's first statement. */
    private fun delegatesToThis(ctor: PsiElement): Boolean {
        val block = ctor.node.findChildByType(E.CODE_BLOCK)?.psi ?: return false
        val first = JuxCodeFacts.statementsOf(block).firstOrNull() ?: return false
        return first.text.trimStart().startsWith("this(")
    }

    private companion object {
        val EXCLUDED = setOf("final", "const", "static", "weak", "volatile")
    }

    /** Writes `final` into the field's modifiers, after its visibility. */
    private class MakeFinalFix : LocalQuickFix {
        override fun getFamilyName(): String = "Make 'final'"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val field = descriptor.psiElement?.parent ?: return
            val list = field.node.findChildByType(E.MODIFIER_LIST)?.psi
            val anchor = list?.let { l ->
                PsiTreeUtil.collectElements(l) { it.elementType === T.PRIVATE_KW }.lastOrNull()
            }
            val file = field.containingFile
            if (anchor != null) {
                JuxCodeFacts.edit(project, file, anchor.textRange.endOffset, anchor.textRange.endOffset, " final")
            } else {
                val start = field.textRange.startOffset
                JuxCodeFacts.edit(project, file, start, start, "final ")
            }
        }
    }
}
