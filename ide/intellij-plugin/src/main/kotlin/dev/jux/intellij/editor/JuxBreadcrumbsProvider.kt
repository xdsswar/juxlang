package dev.jux.intellij.editor

import com.intellij.lang.Language
import com.intellij.psi.PsiElement
import com.intellij.ui.breadcrumbs.BreadcrumbsProvider
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxPropertyDeclaration
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * The breadcrumb trail under the editor: `Parser > Cursor > advance()`.
 *
 * The plugin's own description has promised breadcrumbs since the first
 * release, on the strength of having a real PSI tree — but the provider was
 * never registered, so the strip stayed empty. This makes the claim true.
 *
 * Only declarations are accepted. Including blocks and statements, as some
 * language plugins do, turns the trail into a scroll of `if > for > if` that
 * pushes the enclosing class off the left edge, which is the one thing the
 * trail exists to show.
 */
class JuxBreadcrumbsProvider : BreadcrumbsProvider {

    override fun getLanguages(): Array<Language> = arrayOf(JuxLanguage)

    override fun acceptElement(e: PsiElement): Boolean =
        e.node?.elementType in CRUMB_KINDS && (e as? JuxNamedElement)?.name != null

    override fun getElementInfo(e: PsiElement): String {
        val name = (e as? JuxNamedElement)?.name ?: return ""
        return when (e.node?.elementType) {
            // A method reads better with its parentheses: `advance()` is a
            // method, `advance` could be a field.
            E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                "$name()"
            else -> name
        }
    }

    /**
     * The tooltip names the kind, which is the part the label omits: two
     * crumbs reading `Shape` and `Drawable` look alike until one says
     * "interface".
     */
    override fun getElementTooltip(e: PsiElement): String? = when (e.node?.elementType) {
        E.CLASS_DECLARATION,
        E.INTERFACE_DECLARATION,
        E.ENUM_DECLARATION,
        E.RECORD_DECLARATION,
        E.STRUCT_DECLARATION,
        E.ANNOTATION_DECLARATION,
        E.TYPE_ALIAS_DECLARATION -> (e as? JuxTypeDeclaration)?.let {
            "${JuxHierarchy.kindNoun(it)} ${it.name}"
        }

        E.METHOD_DECLARATION -> JuxHierarchy.methodSignature(e)
        E.CONSTRUCTOR_DECLARATION -> "constructor"
        E.OPERATOR_DECLARATION -> "operator"
        E.PROPERTY_DECLARATION -> (e as? JuxPropertyDeclaration)?.let {
            "property${it.typeText()?.let { t -> ": $t" } ?: ""}"
        }

        E.FIELD_DECLARATION -> "field"
        E.CONST_DECLARATION -> "constant"
        E.ENUM_CONSTANT -> "enum constant"
        else -> null
    }

    private companion object {
        /** Declaration kinds that earn a crumb. */
        val CRUMB_KINDS = setOf(
            E.CLASS_DECLARATION,
            E.INTERFACE_DECLARATION,
            E.ENUM_DECLARATION,
            E.RECORD_DECLARATION,
            E.STRUCT_DECLARATION,
            E.ANNOTATION_DECLARATION,
            E.TYPE_ALIAS_DECLARATION,
            E.METHOD_DECLARATION,
            E.CONSTRUCTOR_DECLARATION,
            E.OPERATOR_DECLARATION,
            E.PROPERTY_DECLARATION,
            E.FIELD_DECLARATION,
            E.CONST_DECLARATION,
            E.ENUM_CONSTANT,
        )
    }
}
