package dev.jux.intellij.psi

import com.intellij.extapi.psi.ASTWrapperPsiElement
import com.intellij.icons.AllIcons
import com.intellij.ide.projectView.PresentationData
import com.intellij.lang.ASTNode
import com.intellij.navigation.ItemPresentation
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiNameIdentifierOwner
import com.intellij.psi.tree.IElementType
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
        JuxElementTypes.PROPERTY_DECLARATION -> AllIcons.Nodes.Property
        JuxElementTypes.ENUM_CONSTANT -> AllIcons.Nodes.Constant
        else -> AllIcons.Nodes.Field
    }
}

/** A top-level or nested type: class / interface / enum / record / struct / annotation. */
class JuxTypeDeclaration(node: ASTNode) : JuxNamedElementImpl(node)

/** A method or free function declaration. */
class JuxMethodDeclaration(node: ASTNode) : JuxNamedElementImpl(node)

/** A field declaration. Open so [JuxPropertyDeclaration] can specialize it. */
open class JuxFieldDeclaration(node: ASTNode) : JuxNamedElementImpl(node)

/**
 * An observable property declaration (§P / §M.7): `Type Name { get; set; } …`
 * or the computed shorthand `Type Name -> expr;`. Subclasses
 * [JuxFieldDeclaration] so every existing field consumer (unused-symbol
 * inspection, Structure View, in-file resolve) keeps treating properties as
 * named members without edits, while §P-aware code can down-match on this type.
 */
class JuxPropertyDeclaration(node: ASTNode) : JuxFieldDeclaration(node) {

    /** The `{ get; set; }` braces — null for the `-> expr;` computed shorthand. */
    fun accessorList(): PsiElement? =
        node.findChildByType(JuxElementTypes.PROPERTY_ACCESSOR_LIST)?.psi

    /** The PROPERTY_ACCESSOR children of the accessor list, in source order. */
    fun accessors(): List<PsiElement> {
        val list = accessorList() ?: return emptyList()
        return list.children.filter { it.elementType == JuxElementTypes.PROPERTY_ACCESSOR }
    }

    /**
     * The accessor's kind — `"get"` / `"set"` — read from its first IDENTIFIER
     * leaf (visibility modifiers are keyword tokens, so the first identifier is
     * always the kind; the removed `init` accessor has none and yields null).
     */
    fun accessorKind(accessor: PsiElement): String? {
        var c: PsiElement? = accessor.firstChild
        while (c != null) {
            if (c.elementType == JuxTokenTypes.IDENTIFIER) return c.text
            c = c.nextSibling
        }
        return null
    }

    fun getterAccessor(): PsiElement? = accessors().firstOrNull { accessorKind(it) == "get" }

    fun setterAccessor(): PsiElement? = accessors().firstOrNull { accessorKind(it) == "set" }

    fun hasSetter(): Boolean = setterAccessor() != null

    /**
     * Read-only computed property (§P.1.5): get-only accessor block or the
     * `-> expr;` shorthand. Computed properties reject assignment (E0970) and
     * never trip the W0971 "never observed" hint.
     */
    fun isComputed(): Boolean = !hasSetter()

    /**
     * The accessor's own visibility keyword token type (`public` / `protected`
     * / `private`), or null when it inherits the property's visibility (§P.1.3).
     */
    fun accessorVisibility(accessor: PsiElement): IElementType? {
        val mods = accessor.node.findChildByType(JuxElementTypes.MODIFIER_LIST) ?: return null
        var c = mods.firstChildNode
        while (c != null) {
            when (c.elementType) {
                JuxTokenTypes.PUBLIC_KW, JuxTokenTypes.PROTECTED_KW, JuxTokenTypes.PRIVATE_KW ->
                    return c.elementType
            }
            c = c.treeNext
        }
        return null
    }

    /** The setter's `{ … }` block body, or null for auto/expression setters. */
    fun setterBody(): PsiElement? =
        setterAccessor()?.node?.findChildByType(JuxElementTypes.CODE_BLOCK)?.psi

    /** The property's declared visibility keyword token type, or null (default). */
    fun propertyVisibility(): IElementType? {
        val mods = node.findChildByType(JuxElementTypes.MODIFIER_LIST) ?: return null
        var c = mods.firstChildNode
        while (c != null) {
            when (c.elementType) {
                JuxTokenTypes.PUBLIC_KW, JuxTokenTypes.PROTECTED_KW, JuxTokenTypes.PRIVATE_KW ->
                    return c.elementType
            }
            c = c.treeNext
        }
        return null
    }

    fun isPublic(): Boolean = propertyVisibility() == JuxTokenTypes.PUBLIC_KW

    fun isPrivate(): Boolean = propertyVisibility() == JuxTokenTypes.PRIVATE_KW

    /** The declared type node (`TYPE_REFERENCE`). */
    fun typeReference(): PsiElement? =
        node.findChildByType(JuxElementTypes.TYPE_REFERENCE)?.psi

    /**
     * The declared type's text with all whitespace stripped — the comparison
     * key for the E0974 bind-type-mismatch check (`List<int>` == `List< int >`).
     */
    fun typeText(): String? = typeReference()?.text?.replace(WS, "")

    private companion object {
        val WS = Regex("\\s+")
    }
}

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
