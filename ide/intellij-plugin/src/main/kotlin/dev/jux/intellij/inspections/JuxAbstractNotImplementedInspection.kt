package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.codeInsight.JuxOverrideMembers
import dev.jux.intellij.psi.JuxAnonymousClass
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.quickfix.JuxMakeClassAbstractFix
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * E0429 mirrored IDE-side: a **non-abstract class** must implement every
 * abstract method it inherits (interface methods without a `default` body,
 * `abstract` methods of an abstract superclass). The Java-plugin red squiggle
 * on the class name, with an "Implement methods" quick-fix that inserts the
 * stubs through the shared [JuxOverrideMembers] engine.
 *
 * The engine resolves supertypes project-wide ([dev.jux.intellij.resolve
 * .JuxTypeIndex]) and stays silent on unresolved names (std / library types),
 * so valid code never gets a false error. A method abstract in an interface
 * but implemented by a nearer concrete ancestor counts as satisfied
 * (nearest-declaration-wins in the engine's walk).
 *
 * An **anonymous class** (`new Shape() { .. }`) owes the same: the compiler
 * lifts it to a named class (ERRATA E137) and reports the lifted class with
 * E0429. It is reported on the `Shape` of the `new`, and named `Shape$anon`,
 * the name the object prints as; only "Implement methods" applies, since an
 * anonymous class cannot be made abstract.
 */
class JuxAbstractNotImplementedInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()

        for (type in PsiTreeUtil.findChildrenOfType(file, JuxTypeDeclaration::class.java)) {
            // Only concrete classes owe implementations; interfaces and
            // abstract classes pass the obligation down. Records/enums can
            // carry implements clauses too — the engine walk covers them the
            // same way.
            if (JuxHierarchy.isInterface(type)) continue
            if (JuxHierarchy.isAbstractType(type)) continue
            val anonymousOf = (type as? JuxAnonymousClass)?.supertypeReference()
            val name = type.name ?: anonymousOf?.let { "${it.text.substringBefore('<').substringAfterLast('.').trim()}\$anon" } ?: continue
            val target = type.nameIdentifier ?: anonymousOf ?: continue
            // A class extending a Rust type is E0420 and only that (ERRATA
            // E135): the type's methods are the crate's, not ones to implement.
            if (extendsRustType(type)) continue

            val missing = JuxOverrideMembers.candidates(type)
                .filter { it.kind == JuxOverrideMembers.Kind.IMPLEMENT }
            if (missing.isEmpty()) continue

            // Same sentence shape as the compiler's E0429.
            val list = missing
                .map { "'${it.ownerName}.${it.method.name}'" }
                .distinct()
                .sorted()
                .joinToString(", ")
            // Java's pair: implement the methods, or pass the obligation on by
            // making the class abstract (only a `class` can be abstract).
            val fixes = ArrayList<com.intellij.codeInspection.LocalQuickFix>()
            fixes.add(ImplementMethodsFix())
            if (JuxHierarchy.isClass(type)) fixes.add(JuxMakeClassAbstractFix(type))
            problems.add(
                manager.createProblemDescriptor(
                    target,
                    "Class '$name' doesn't implement abstract method(s): $list (E0429)",
                    isOnTheFly,
                    fixes.toTypedArray(),
                    ProblemHighlightType.ERROR,
                ),
            )
        }
        return problems.toTypedArray()
    }

    /** Whether [type]'s `extends` clause names a Rust type, anywhere up its chain. */
    private fun extendsRustType(type: JuxTypeDeclaration): Boolean {
        val seen = HashSet<JuxTypeDeclaration>()
        var current: JuxTypeDeclaration? = type
        while (current != null && seen.add(current)) {
            val ref = JuxHierarchy.supertypeReferences(current).firstOrNull { it.second }?.first ?: return false
            val parent = JuxTypeIndex.findTypeThroughAliases(ref, JuxHierarchy.bareTypeName(ref)) ?: return false
            if (JuxHierarchy.isRustType(parent)) return true
            current = parent
        }
        return false
    }

    /**
     * Inserts stubs for ALL missing abstract methods (no chooser inside a
     * quick-fix); candidates are recomputed in [applyFix] so the descriptor
     * never applies stale PSI.
     */
    private class ImplementMethodsFix : LocalQuickFix {
        override fun getFamilyName(): String = "Implement methods"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val at = descriptor.psiElement ?: return
            // A class's name sits in the class; an anonymous class's `Shape`
            // sits in the `new` beside it.
            val type = at.parent as? JuxTypeDeclaration
                ?: PsiTreeUtil.getChildOfType(at.parent, JuxAnonymousClass::class.java)
                ?: return
            val missing = JuxOverrideMembers.candidates(type)
                .filter { it.kind == JuxOverrideMembers.Kind.IMPLEMENT }
            JuxOverrideMembers.insertStubs(project, type, missing)
        }
    }
}
