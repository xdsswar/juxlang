package dev.jux.intellij.resolve

import com.intellij.lang.cacheBuilder.DefaultWordsScanner
import com.intellij.lang.cacheBuilder.WordsScanner
import com.intellij.lang.findUsages.FindUsagesProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiNamedElement
import com.intellij.psi.tree.TokenSet
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxLexer
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/** Enables Find Usages for Jux named declarations and describes them. */
class JuxFindUsagesProvider : FindUsagesProvider {
    override fun getWordsScanner(): WordsScanner =
        DefaultWordsScanner(
            JuxLexer(),
            TokenSet.create(JuxTokenTypes.IDENTIFIER),
            JuxTokenTypes.COMMENTS,
            JuxTokenTypes.LITERALS,
        )

    override fun canFindUsagesFor(element: PsiElement): Boolean = element is JuxNamedElement

    override fun getHelpId(element: PsiElement): String? = null

    override fun getType(element: PsiElement): String = when (element.elementType) {
        E.CLASS_DECLARATION, E.STRUCT_DECLARATION -> "class"
        E.INTERFACE_DECLARATION -> "interface"
        E.ENUM_DECLARATION -> "enum"
        E.RECORD_DECLARATION -> "record"
        E.ANNOTATION_DECLARATION -> "annotation"
        E.METHOD_DECLARATION, E.OPERATOR_DECLARATION -> "method"
        E.CONSTRUCTOR_DECLARATION -> "constructor"
        E.FIELD_DECLARATION, E.PROPERTY_DECLARATION, E.CONST_DECLARATION -> "field"
        E.ENUM_CONSTANT -> "enum constant"
        else -> "declaration"
    }

    override fun getDescriptiveName(element: PsiElement): String =
        (element as? PsiNamedElement)?.name.orEmpty()

    override fun getNodeText(element: PsiElement, useFullName: Boolean): String =
        getDescriptiveName(element)
}
