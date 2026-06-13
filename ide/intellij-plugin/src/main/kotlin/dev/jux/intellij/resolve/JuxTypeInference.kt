package dev.jux.intellij.resolve

import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * Lightweight, in-file type inference for **member completion** (`recv.<caret>`)
 * — the IDE-side approximation of what `juxc-lsp` does with the full type
 * checker. It resolves a receiver to the type whose members should be offered,
 * for the common, statically-obvious shapes:
 *
 *  - `this` / `super` → the enclosing type (or its `extends` parent);
 *  - a local / parameter / field / property whose declared type is written
 *    (`Point p`, `Point field;`) or inferable from a `new T(...)` initializer
 *    (`var p = new Point();`);
 *  - a bare **type name** (`Color.`, `Math.`) → that type, in *static* mode.
 *
 * It deliberately does NOT attempt full expression typing (chained calls,
 * generics substitution, stdlib/Rust types) — those stay with the LSP. Every
 * lookup is project-wide via [JuxTypeIndex], so cross-file user types resolve.
 */
object JuxTypeInference {

    /** The type a receiver denotes, plus whether the access is static. */
    data class Target(val type: JuxTypeDeclaration, val isStatic: Boolean)

    /**
     * Resolve the receiver named [receiverWord] (the identifier immediately
     * before the `.`) as seen from [context] (the PSI element at the caret).
     * Returns null when the type can't be determined in-file — the caller then
     * offers nothing (member completion is the LSP's job for those).
     */
    fun resolveReceiver(receiverWord: String, context: PsiElement): Target? {
        val project = context.project

        // `this` / `super` — the enclosing type, or its extends parent.
        if (receiverWord == "this" || receiverWord == "super") {
            val enclosing = PsiTreeUtil.getParentOfType(context, JuxTypeDeclaration::class.java) ?: return null
            if (receiverWord == "super") {
                val parentName = JuxHierarchy.superTypeNames(enclosing).firstOrNull() ?: return null
                val parent = JuxTypeIndex.findType(project, parentName) ?: return null
                return Target(parent, isStatic = false)
            }
            return Target(enclosing, isStatic = false)
        }

        // A value declaration (local / param / field / property) visible here?
        val decl = resolveValueDecl(receiverWord, context)
        if (decl != null) {
            val typeName = declaredTypeName(decl) ?: return null
            val type = JuxTypeIndex.findType(project, typeName) ?: return null
            return Target(type, isStatic = false)
        }

        // Otherwise the word may name a TYPE → static-member access.
        val type = JuxTypeIndex.findType(project, receiverWord) ?: return null
        return Target(type, isStatic = true)
    }

    /**
     * Find the value declaration named [name] visible from [context], walking
     * enclosing scopes innermost-out — same shape as
     * [JuxReference.resolveLocally] but restricted to value (non-type)
     * declarations: locals, parameters, fields, properties.
     */
    private fun resolveValueDecl(name: String, context: PsiElement): JuxNamedElement? {
        val offset = context.textOffset
        var scope: PsiElement? = context.parent
        while (scope != null) {
            when (scope.elementType) {
                E.CODE_BLOCK ->
                    for (child in scope.children) {
                        if (child.elementType === E.LOCAL_VARIABLE && child.textOffset < offset &&
                            (child as? JuxNamedElement)?.name == name
                        ) return child as JuxNamedElement
                    }
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                    scope.children.firstOrNull { it.elementType === E.PARAMETER_LIST }
                        ?.children?.forEach { p ->
                            if (p.elementType === E.PARAMETER && (p as? JuxNamedElement)?.name == name) {
                                return p as JuxNamedElement
                            }
                        }
                E.CLASS_BODY ->
                    for (m in scope.children) {
                        if ((m.elementType === E.FIELD_DECLARATION || m.elementType === E.PROPERTY_DECLARATION) &&
                            (m as? JuxNamedElement)?.name == name
                        ) return m as JuxNamedElement
                    }
            }
            if (scope is JuxFile) break
            scope = scope.parent
        }
        return null
    }

    /**
     * The bare type name a value declaration introduces: the written
     * `TYPE_REFERENCE` if present, else inferred from a `new T(...)`
     * initializer on a `var` local. Returns null when no type is recoverable.
     */
    private fun declaredTypeName(decl: JuxNamedElement): String? {
        val node = (decl as PsiElement)
        // Explicit type: `Point p`, `Point field;`, `Point Prop { get; set; }`.
        node.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let { return bareName(it) }
        // `var p = new Point();` — infer from the initializer's new-expression.
        if (node.elementType === E.LOCAL_VARIABLE) {
            val newExpr = PsiTreeUtil.findChildrenOfType(node, PsiElement::class.java)
                .firstOrNull { it.elementType === E.NEW_EXPRESSION } ?: return null
            newExpr.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let { return bareName(it) }
        }
        return null
    }

    /** Last segment of a TYPE_REFERENCE, generics stripped (`a.b.List<int>` → `List`). */
    private fun bareName(typeRef: PsiElement): String =
        typeRef.text.trim().substringAfterLast('.').substringBefore('<').trim()
}
