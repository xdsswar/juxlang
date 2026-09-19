package dev.jux.intellij.codeInsight

import com.intellij.codeInsight.navigation.actions.TypeDeclarationProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxMember
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * Navigate | Type Declaration (Ctrl+Shift+B), as Java has it: from a
 * variable, parameter, field, property or record component to the
 * declaration of its type, and from a method to the declaration of what it
 * returns. `Box<Truck> b` goes to `Box`; a `Truck?` or `Truck[]` goes to
 * `Truck`; a primitive has no declaration to go to.
 */
class JuxTypeDeclarationProvider : TypeDeclarationProvider {

    override fun getSymbolTypeDeclarations(symbol: PsiElement): Array<PsiElement>? {
        if (symbol.language != JuxLanguage) return null
        val type = when (symbol.elementType) {
            E.LOCAL_VARIABLE, E.PARAMETER, E.FIELD_DECLARATION, E.CONST_DECLARATION,
            E.PROPERTY_DECLARATION, E.RECORD_COMPONENT, E.ENUM_CONSTANT,
            -> JuxTypeEngine.declaredType(symbol)
            E.METHOD_DECLARATION -> {
                val owner = PsiTreeUtil.getParentOfType(symbol, JuxTypeDeclaration::class.java)
                if (owner != null) JuxTypeEngine.returnType(JuxMember(symbol, JuxTypeEngine.selfType(owner)))
                else JuxTypeEngine.declaredType(symbol)
            }
            else -> return null
        }
        val decl = declarationOf(type) ?: return null
        return arrayOf(decl)
    }

    /** The declaration a type names, looking through `?` and `[]`. */
    private fun declarationOf(type: JuxType): PsiElement? = when (type) {
        is JuxType.Nullable -> declarationOf(type.inner)
        is JuxType.ArrayType -> declarationOf(type.element)
        is JuxType.TypeVar -> type.param
        else -> JuxTypeEngine.classOf(type)?.decl
    }
}
