package dev.jux.intellij.codeInsight

import com.intellij.codeInsight.navigation.PsiTargetNavigator
import com.intellij.lang.LanguageCodeInsightActionHandler
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.pom.Navigatable
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * Go to Super Method (Ctrl+U), the way Java's `JavaGotoSuperHandler` works:
 *
 * - with the caret in a method, jump to the method it overrides or implements.
 *   Each direct supertype contributes its nearest declaration, so a class that
 *   both extends a base and implements an interface declaring the same method
 *   offers both, in a chooser;
 * - anywhere else in a type, jump to its supertypes.
 *
 * Methods match by name and arity, the approximation [JuxHierarchy] uses for
 * every override question (the override gutter, Ctrl+O, the missing-`@Override`
 * inspection), so this and those always agree.
 */
class JuxGotoSuperHandler : LanguageCodeInsightActionHandler {

    override fun isValidFor(editor: Editor?, file: PsiFile?): Boolean = file is JuxFile

    override fun startInWriteAction(): Boolean = false

    override fun invoke(project: Project, editor: Editor, file: PsiFile) {
        val at = file.findElementAt(editor.caretModel.offset) ?: return
        val found = targets(at)
        when (found.size) {
            0 -> return
            1 -> (found[0] as? Navigatable)?.navigate(true)
            else -> PsiTargetNavigator(found).navigate(editor, "Choose Super")
        }
    }

    companion object {
        /**
         * Where Ctrl+U goes from [at]: the super methods of the enclosing
         * method, or the supertypes of the enclosing type when the caret is
         * not in a method (or the method overrides nothing, as Java's does).
         */
        fun targets(at: PsiElement): List<PsiElement> {
            val method = PsiTreeUtil.getParentOfType(at, JuxMethodDeclaration::class.java, false)
            if (method != null && method.elementType === E.METHOD_DECLARATION) {
                val supers = superMethods(method)
                if (supers.isNotEmpty()) return supers
            }
            val type = PsiTreeUtil.getParentOfType(at, JuxTypeDeclaration::class.java, false) ?: return emptyList()
            return superTypes(type)
        }

        /**
         * The methods [method] overrides: for each direct supertype, the
         * nearest declaration with the same name and arity along that branch.
         */
        fun superMethods(method: JuxMethodDeclaration): List<PsiElement> {
            val type = JuxHierarchy.enclosingType(method) ?: return emptyList()
            val name = method.name ?: return emptyList()
            // A static method hides; it never overrides (§7.4.1).
            if (JuxHierarchy.hasModifier(method, "static")) return emptyList()
            val arity = JuxHierarchy.arity(method)
            val out = LinkedHashSet<PsiElement>()
            for (superType in superTypes(type)) {
                val own = JuxHierarchy.directChildren(superType, E.METHOD_DECLARATION).firstOrNull {
                    (it as? JuxNamedElement)?.name == name && JuxHierarchy.arity(it) == arity &&
                        JuxHierarchy.isOverridable(it)
                }
                (own ?: JuxHierarchy.findSuperMethod(superType, name, arity))?.let(out::add)
            }
            return out.toList()
        }

        /** The resolvable direct supertypes of [type], `extends` first. */
        fun superTypes(type: JuxTypeDeclaration): List<JuxTypeDeclaration> =
            JuxHierarchy.superTypeNames(type).mapNotNull { JuxTypeIndex.findType(type, it) }
    }
}
