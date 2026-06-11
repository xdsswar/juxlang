package dev.jux.intellij.highlight

import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.Annotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.resolve.JuxReference

/**
 * PSI-based semantic highlighting — the layer the flat lexer can't provide.
 * Colours, off the parse tree:
 *
 * - **primitive type names** (`int`, `i32`, `String`, …) — identifiers at the
 *   lexer level, recognised here by position (inside a type reference) + the
 *   compiler-shared [JuxKeywords.PRIMITIVES] set;
 * - **annotation names** — the identifier(s) after `@`;
 * - **declaration names** — the class/interface/enum, method, and field name
 *   identifiers, each in its own colour (Java-like "the name stands out");
 * - **reference uses** (decl-vs-use colouring): call sites, parameter/local
 *   reads, field reads, type-parameter uses, enum constants, and class-name
 *   references — classified by position (call shape, type position) plus the
 *   **in-file** resolver ([JuxReference.resolveLocally]). Cross-file resolve
 *   is deliberately never invoked from here: it scans the whole project, and
 *   the annotator runs per identifier on every editor pass.
 */
class JuxAnnotator : Annotator {
    override fun annotate(element: PsiElement, holder: AnnotationHolder) {
        if (element.elementType !== JuxTokenTypes.IDENTIFIER) return
        val key = colorFor(element) ?: return
        holder.newSilentAnnotation(HighlightSeverity.INFORMATION)
            .range(element)
            .textAttributes(key)
            .create()
    }

    private fun colorFor(id: PsiElement): TextAttributesKey? {
        val parent = id.parent ?: return null

        // Declaration name: the name identifier of a class/method/field/etc.
        val named = parent as? JuxNamedElement
        if (named != null && named.nameIdentifier === id) {
            return when (parent.elementType) {
                E.CLASS_DECLARATION, E.INTERFACE_DECLARATION, E.ENUM_DECLARATION,
                E.RECORD_DECLARATION, E.STRUCT_DECLARATION, E.ANNOTATION_DECLARATION,
                E.TYPE_ALIAS_DECLARATION -> JuxSyntaxHighlighter.CLASS_NAME
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                    JuxSyntaxHighlighter.METHOD_DECLARATION
                E.FIELD_DECLARATION, E.CONST_DECLARATION, E.PROPERTY_DECLARATION ->
                    JuxSyntaxHighlighter.FIELD
                E.ENUM_CONSTANT -> JuxSyntaxHighlighter.ENUM_CONSTANT
                E.PARAMETER -> JuxSyntaxHighlighter.PARAMETER
                E.LOCAL_VARIABLE -> JuxSyntaxHighlighter.LOCAL_VARIABLE
                else -> null
            }
        }

        // Annotation name: `@Name` / `@a.b.C`.
        if (parent.elementType === E.QUALIFIED_NAME && parent.parent?.elementType === E.ANNOTATION) {
            return JuxSyntaxHighlighter.ANNOTATION
        }

        // Type position: primitive names, type-parameter uses, class names.
        if (parent.elementType === E.TYPE_REFERENCE) {
            val name = id.text
            if (name in JuxKeywords.PRIMITIVES) return JuxSyntaxHighlighter.TYPE
            if (isEnclosingTypeParameter(id, name)) return JuxSyntaxHighlighter.TYPE_PARAMETER
            // A capitalized name in type position is a class reference whether
            // or not it resolves in-file — cross-file/std types look right
            // without paying for an index scan.
            if (name.firstOrNull()?.isUpperCase() == true) return JuxSyntaxHighlighter.CLASS_NAME
            return null
        }

        // Type-parameter *declarations* (`class Box<T>` — the `T`): the parser
        // wraps them in TYPE_PARAMETER nodes, which aren't named elements.
        if (parent.elementType === E.TYPE_PARAMETER) {
            return JuxSyntaxHighlighter.TYPE_PARAMETER
        }

        // Reference uses: classify by call shape first (works without resolve,
        // so cross-file calls colour correctly too), then by in-file resolve.
        if (parent.elementType in REFERENCE_PARENTS) {
            if (isCallPosition(id, parent)) return JuxSyntaxHighlighter.METHOD_CALL

            // The reference lives on the composite node, ranged over its name
            // leaf — only colour when that leaf is THIS identifier (so a
            // qualifier never borrows the member's resolution).
            val ref = parent.references.firstOrNull() as? JuxReference ?: return null
            if (ref.rangeInElement.startOffset != id.startOffsetInParent) return null
            val resolved = ref.resolveLocally() ?: return null
            return when (resolved.elementType) {
                E.CLASS_DECLARATION, E.INTERFACE_DECLARATION, E.ENUM_DECLARATION,
                E.RECORD_DECLARATION, E.STRUCT_DECLARATION, E.ANNOTATION_DECLARATION,
                E.TYPE_ALIAS_DECLARATION -> JuxSyntaxHighlighter.CLASS_NAME
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                    JuxSyntaxHighlighter.METHOD_CALL
                E.FIELD_DECLARATION, E.CONST_DECLARATION, E.PROPERTY_DECLARATION ->
                    JuxSyntaxHighlighter.FIELD
                E.ENUM_CONSTANT -> JuxSyntaxHighlighter.ENUM_CONSTANT
                E.PARAMETER -> JuxSyntaxHighlighter.PARAMETER
                E.LOCAL_VARIABLE -> JuxSyntaxHighlighter.LOCAL_VARIABLE
                else -> null
            }
        }

        return null
    }

    /**
     * True when [id] is the *called name* of a call: the callee of a
     * `CALL_EXPRESSION` (`foo(…)`) or the member name of a field access that
     * is itself being called (`obj.method(…)` — the access node is the
     * call's callee and [id] is its last identifier).
     */
    private fun isCallPosition(id: PsiElement, parent: PsiElement): Boolean {
        val grand = parent.parent ?: return false
        if (grand.elementType !== E.CALL_EXPRESSION) return false
        // The callee is the call's first child; an identifier inside an
        // ARGUMENT_LIST must not be classified as the called name.
        if (grand.firstChild !== parent) return false
        return when (parent.elementType) {
            E.REFERENCE_EXPRESSION -> true
            // `obj.method(…)` — only the trailing name is the method.
            E.FIELD_ACCESS_EXPRESSION, E.METHOD_REF_EXPRESSION -> isLastIdentifier(id, parent)
            else -> false
        }
    }

    /** True when [id] is the last IDENTIFIER leaf directly under [parent]. */
    private fun isLastIdentifier(id: PsiElement, parent: PsiElement): Boolean {
        var last: PsiElement? = null
        var c: PsiElement? = parent.firstChild
        while (c != null) {
            if (c.elementType === JuxTokenTypes.IDENTIFIER) last = c
            c = c.nextSibling
        }
        return last === id
    }

    /**
     * True when [name] matches a type parameter declared by an enclosing
     * method or type declaration (`class Box<T>` / `<T> T id(T x)`).
     */
    private fun isEnclosingTypeParameter(at: PsiElement, name: String): Boolean {
        var scope: PsiElement? = at.parent
        while (scope != null) {
            val t = scope.elementType
            if (t === E.CLASS_DECLARATION || t === E.INTERFACE_DECLARATION ||
                t === E.ENUM_DECLARATION || t === E.RECORD_DECLARATION ||
                t === E.STRUCT_DECLARATION || t === E.METHOD_DECLARATION ||
                t === E.TYPE_ALIAS_DECLARATION
            ) {
                val params = scope.node.findChildByType(E.TYPE_PARAMETER_LIST)?.psi
                if (params != null) {
                    var p: PsiElement? = params.firstChild
                    while (p != null) {
                        if (p.elementType === E.TYPE_PARAMETER &&
                            p.firstIdentifierText() == name
                        ) return true
                        p = p.nextSibling
                    }
                }
            }
            scope = scope.parent
        }
        return false
    }

    private fun PsiElement.firstIdentifierText(): String? {
        var c: PsiElement? = firstChild
        while (c != null) {
            if (c.elementType === JuxTokenTypes.IDENTIFIER) return c.text
            c = c.nextSibling
        }
        return null
    }

    private companion object {
        val REFERENCE_PARENTS = setOf(
            E.REFERENCE_EXPRESSION,
            E.FIELD_ACCESS_EXPRESSION,
            E.METHOD_REF_EXPRESSION,
        )
    }
}
