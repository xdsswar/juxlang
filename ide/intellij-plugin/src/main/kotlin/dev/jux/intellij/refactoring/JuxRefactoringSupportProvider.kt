package dev.jux.intellij.refactoring

import com.intellij.lang.refactoring.RefactoringSupportProvider
import com.intellij.psi.PsiElement
import com.intellij.refactoring.RefactoringActionHandler
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxLocalVariable
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxParameter

/**
 * The gate on the Refactor menu.
 *
 * Until this provider was registered the menu held Rename and nothing else, no
 * matter what handlers existed — the platform asks *this* whether a language
 * supports refactoring at all before it enables Extract Variable, Introduce
 * Constant or Safe Delete.
 */
class JuxRefactoringSupportProvider : RefactoringSupportProvider() {

    override fun isAvailable(context: PsiElement): Boolean = context.containingFile is JuxFile

    /**
     * Safe Delete is offered for any named declaration, since the usage search
     * behind it ([JuxSafeDeleteProcessor]) handles them all uniformly.
     */
    override fun isSafeDeleteAvailable(element: PsiElement): Boolean =
        element is JuxNamedElement && element.containingFile is JuxFile

    override fun getIntroduceVariableHandler(): RefactoringActionHandler = JuxIntroduceVariableHandler()

    override fun getIntroduceVariableHandler(element: PsiElement?): RefactoringActionHandler =
        JuxIntroduceVariableHandler()

    override fun getIntroduceConstantHandler(): RefactoringActionHandler = JuxIntroduceConstantHandler()

    /**
     * In-place rename (type the new name in the editor, no dialog) for the
     * bindings whose every use is in the same file by construction. A member or
     * a type can be referenced from anywhere in the project, so those keep the
     * dialog, which is where the "search in comments" options live.
     */
    override fun isInplaceRenameAvailable(element: PsiElement, context: PsiElement?): Boolean =
        element is JuxLocalVariable || element is JuxParameter
}
