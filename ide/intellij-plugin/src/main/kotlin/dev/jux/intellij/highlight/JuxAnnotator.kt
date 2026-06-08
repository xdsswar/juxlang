package dev.jux.intellij.highlight

import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.Annotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/**
 * PSI-based semantic highlighting — the layer the flat lexer can't provide.
 * Colours, off the parse tree:
 *
 * - **primitive type names** (`int`, `i32`, `String`, …) — identifiers at the
 *   lexer level, recognised here by position (inside a type reference) + the
 *   compiler-shared [JuxKeywords.PRIMITIVES] set;
 * - **annotation names** — the identifier(s) after `@`;
 * - **declaration names** — the class/interface/enum, method, and field name
 *   identifiers, each in its own colour (Java-like "the name stands out").
 *
 * Reference colouring (decl-vs-use) needs name resolution and lands with the
 * in-project resolver (Phase 5); this layer only colours definitions and
 * positionally-unambiguous names, so it needs no resolve and is `DumbAware`-safe.
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
                E.FIELD_DECLARATION, E.CONST_DECLARATION, E.PROPERTY_DECLARATION, E.ENUM_CONSTANT ->
                    JuxSyntaxHighlighter.FIELD
                else -> null
            }
        }

        // Annotation name: `@Name` / `@a.b.C`.
        if (parent.elementType === E.QUALIFIED_NAME && parent.parent?.elementType === E.ANNOTATION) {
            return JuxSyntaxHighlighter.ANNOTATION
        }

        // Primitive type name inside a type reference.
        if (parent.elementType === E.TYPE_REFERENCE && id.text in JuxKeywords.PRIMITIVES) {
            return JuxSyntaxHighlighter.TYPE
        }

        return null
    }
}
