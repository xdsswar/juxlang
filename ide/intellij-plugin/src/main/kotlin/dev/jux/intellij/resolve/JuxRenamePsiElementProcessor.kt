package dev.jux.intellij.resolve

import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import com.intellij.refactoring.rename.RenamePsiElementProcessor
import com.intellij.util.containers.MultiMap
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/**
 * Rename support for Jux declarations. Usage collection itself is handled by the
 * platform — in-file uses resolve through [JuxReference] (surfaced by the
 * reference contributor), so they are found and rewritten automatically. What
 * this processor adds is **conflict detection**: renaming a declaration onto a
 * name already taken by a sibling in the same scope is reported up-front (the
 * refactoring's "Conflicts" dialog) instead of silently producing two
 * same-named declarations.
 *
 * Cross-file rename is intentionally out of scope for now (the per-file resolver
 * only resolves types across files); same-file renames are fully covered.
 */
class JuxRenamePsiElementProcessor : RenamePsiElementProcessor() {

    override fun canProcessElement(element: PsiElement): Boolean = element is JuxNamedElement

    override fun findExistingNameConflicts(
        element: PsiElement,
        newName: String,
        conflicts: MultiMap<PsiElement, String>,
    ) {
        if (element !is JuxNamedElement) return
        val clash = siblingNamed(element, newName) ?: return
        conflicts.putValue(
            clash,
            "A ${describe(clash)} named '$newName' is already declared in this scope",
        )
    }

    /**
     * The first declaration sharing [element]'s immediate scope (same code block
     * for locals, same parameter list for parameters, same class body for
     * members, same file for top-level types) that already bears [newName].
     * Nested-block shadowing is allowed in Jux, so only the *same* container is
     * inspected — an outer-scope match is not a conflict.
     */
    private fun siblingNamed(element: JuxNamedElement, newName: String): JuxNamedElement? {
        // Methods / constructors / operators are overloadable (§T.3 type-based
        // overloading), so a same-name sibling is a legal overload, not a clash —
        // never report a conflict for or against those. Locals, parameters,
        // fields, types and enum constants are unique within their scope.
        if (element.elementType in OVERLOADABLE) return null
        val container = element.parent ?: return null
        return container.children.firstOrNull { sib ->
            sib !== element && sib is JuxNamedElement &&
                sib.elementType !in OVERLOADABLE && sib.name == newName
        } as? JuxNamedElement
    }

    /** A short noun for the conflicting declaration, for the conflict message. */
    private fun describe(decl: JuxNamedElement): String = when (decl.elementType) {
        E.CLASS_DECLARATION, E.STRUCT_DECLARATION -> "class"
        E.INTERFACE_DECLARATION -> "interface"
        E.ENUM_DECLARATION -> "enum"
        E.RECORD_DECLARATION -> "record"
        E.METHOD_DECLARATION, E.OPERATOR_DECLARATION -> "method"
        E.CONSTRUCTOR_DECLARATION -> "constructor"
        E.FIELD_DECLARATION, E.CONST_DECLARATION, E.PROPERTY_DECLARATION -> "field"
        E.ENUM_CONSTANT -> "enum constant"
        E.PARAMETER -> "parameter"
        E.LOCAL_VARIABLE -> "variable"
        else -> "declaration"
    }

    private companion object {
        /** Member kinds that may legally share a name (overloads). */
        val OVERLOADABLE = setOf(
            E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION,
        )
    }
}
