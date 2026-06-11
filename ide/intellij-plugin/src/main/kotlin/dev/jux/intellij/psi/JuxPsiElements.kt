package dev.jux.intellij.psi

import com.intellij.extapi.psi.ASTWrapperPsiElement
import com.intellij.icons.AllIcons
import com.intellij.ide.projectView.PresentationData
import com.intellij.lang.ASTNode
import com.intellij.navigation.ItemPresentation
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiNameIdentifierOwner
import com.intellij.psi.util.elementType
import com.intellij.util.IncorrectOperationException
import dev.jux.intellij.highlight.JuxTokenTypes
import javax.swing.Icon

/**
 * A Jux declaration that introduces a name (class, method, field, …). Mixes in
 * the platform's [PsiNameIdentifierOwner] so rename, Find Usages, and the
 * Structure View work off the identifier token.
 */
interface JuxNamedElement : PsiNameIdentifierOwner

/** Generic backing element for composite nodes that need no special behaviour. */
open class JuxCompositeElement(node: ASTNode) : ASTWrapperPsiElement(node) {
    /**
     * Surface provider-contributed references ([dev.jux.intellij.resolve.
     * JuxReferenceContributor]). Custom-language PSI does NOT consult the
     * provider registry by default — without this override, contributed
     * references are invisible to `findReferenceAt`, rename, and Find Usages.
     */
    override fun getReferences(): Array<com.intellij.psi.PsiReference> =
        com.intellij.psi.impl.source.resolve.reference.ReferenceProvidersRegistry
            .getReferencesFromProviders(this)
}

/**
 * Base for named declarations. The name is the declaration's first
 * [JuxTokenTypes.IDENTIFIER] leaf (which follows the declaration keyword, e.g.
 * `class` `Foo`). [setName] swaps that leaf for a freshly-minted one so the
 * platform's Rename refactoring can update the declaration.
 */
abstract class JuxNamedElementImpl(node: ASTNode) : JuxCompositeElement(node), JuxNamedElement {
    override fun getNameIdentifier(): PsiElement? {
        var child: PsiElement? = firstChild
        while (child != null) {
            if (child.elementType == JuxTokenTypes.IDENTIFIER) return child
            child = child.nextSibling
        }
        return null
    }

    override fun getName(): String? = nameIdentifier?.text

    override fun setName(name: String): PsiElement {
        val id = nameIdentifier
            ?: throw IncorrectOperationException("declaration has no name to rename")
        id.replace(JuxElementFactory.createIdentifier(project, name))
        return this
    }

    /**
     * Name + containing-file location + node icon — what the Go-to-Class /
     * Go-to-Symbol popups and the navigation bar render for this declaration.
     */
    override fun getPresentation(): ItemPresentation =
        PresentationData(name, containingFile?.name?.let { "($it)" }, presentationIcon(), null)

    private fun presentationIcon(): Icon = when (elementType) {
        JuxElementTypes.INTERFACE_DECLARATION -> AllIcons.Nodes.Interface
        JuxElementTypes.ENUM_DECLARATION -> AllIcons.Nodes.Enum
        JuxElementTypes.RECORD_DECLARATION -> AllIcons.Nodes.Record
        JuxElementTypes.ANNOTATION_DECLARATION -> AllIcons.Nodes.Annotationtype
        JuxElementTypes.CLASS_DECLARATION, JuxElementTypes.STRUCT_DECLARATION,
        JuxElementTypes.TYPE_ALIAS_DECLARATION -> AllIcons.Nodes.Class
        JuxElementTypes.METHOD_DECLARATION, JuxElementTypes.CONSTRUCTOR_DECLARATION,
        JuxElementTypes.OPERATOR_DECLARATION -> AllIcons.Nodes.Method
        JuxElementTypes.ENUM_CONSTANT -> AllIcons.Nodes.Constant
        else -> AllIcons.Nodes.Field
    }
}

/** A top-level or nested type: class / interface / enum / record / struct / annotation. */
class JuxTypeDeclaration(node: ASTNode) : JuxNamedElementImpl(node)

/** A method or free function declaration. */
class JuxMethodDeclaration(node: ASTNode) : JuxNamedElementImpl(node)

/** A field declaration. */
class JuxFieldDeclaration(node: ASTNode) : JuxNamedElementImpl(node)

/** An enum constant. */
class JuxEnumConstant(node: ASTNode) : JuxNamedElementImpl(node)

/**
 * A method/constructor parameter. The name is the identifier *after* the type —
 * the **last** direct identifier leaf — because a `final`/`out` param-mode is
 * itself an identifier-shaped leaf that precedes the type, and the type's own
 * name is nested inside a `TYPE_REFERENCE` (so never a direct child here).
 */
class JuxParameter(node: ASTNode) : JuxNamedElementImpl(node) {
    override fun getNameIdentifier(): PsiElement? {
        var last: PsiElement? = null
        var child: PsiElement? = firstChild
        while (child != null) {
            if (child.elementType == JuxTokenTypes.IDENTIFIER) last = child
            child = child.nextSibling
        }
        return last
    }
}

/** A local variable declaration. */
class JuxLocalVariable(node: ASTNode) : JuxNamedElementImpl(node)
