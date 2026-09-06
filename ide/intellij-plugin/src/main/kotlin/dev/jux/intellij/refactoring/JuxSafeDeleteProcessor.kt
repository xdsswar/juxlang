package dev.jux.intellij.refactoring

import com.intellij.openapi.project.Project
import com.intellij.openapi.util.Condition
import com.intellij.psi.PsiElement
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.searches.ReferencesSearch
import com.intellij.refactoring.safeDelete.NonCodeUsageSearchInfo
import com.intellij.refactoring.safeDelete.SafeDeleteProcessorDelegate
import com.intellij.refactoring.safeDelete.usageInfo.SafeDeleteReferenceSimpleDeleteUsageInfo
import com.intellij.usageView.UsageInfo
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement

/**
 * Safe Delete for Jux declarations — find who still uses this, show them, and
 * only then remove it.
 *
 * Every named Jux declaration is handled: a type, method, property, field,
 * constant, enum constant, parameter, type parameter or local. The usage search
 * is the plugin's own reference machinery (`JuxReferenceContributor` feeding
 * `ReferencesSearch`), which is the same search Find Usages runs — so what the
 * conflicts dialog lists is exactly what Alt+F7 would have shown.
 */
class JuxSafeDeleteProcessor : SafeDeleteProcessorDelegate {

    override fun handlesElement(element: PsiElement): Boolean =
        element is JuxNamedElement && element.containingFile is JuxFile

    override fun findUsages(
        element: PsiElement,
        allElementsToDelete: Array<out PsiElement>,
        result: MutableList<in UsageInfo>,
    ): NonCodeUsageSearchInfo {
        val doomed = allElementsToDelete.toSet()
        val insideDeleted = Condition<PsiElement> { candidate ->
            doomed.any { com.intellij.psi.util.PsiTreeUtil.isAncestor(it, candidate, false) }
        }

        ReferencesSearch.search(element, GlobalSearchScope.projectScope(element.project)).forEach { reference ->
            val usage = reference.element
            // A reference that lives inside something already being deleted is
            // not an obstacle: it is about to go too. Reporting it would make
            // deleting a class and its only user look unsafe.
            if (!insideDeleted.value(usage)) {
                result.add(SafeDeleteReferenceSimpleDeleteUsageInfo(usage, element, false))
            }
            true
        }
        return NonCodeUsageSearchInfo(insideDeleted, element)
    }

    /** The declaration itself is the only thing to search for. */
    override fun getElementsToSearch(
        element: PsiElement,
        allElementsToDelete: MutableCollection<out PsiElement>,
    ): Collection<PsiElement> = listOf(element)

    /**
     * Nothing is deleted alongside the target.
     *
     * Java pulls in overriding methods here. Jux could eventually do the same,
     * but silently deleting code in files the user did not open is exactly the
     * surprise Safe Delete exists to prevent, so it stays opt-in-by-hand until
     * there is a dialog to opt in with.
     */
    override fun getAdditionalElementsToDelete(
        element: PsiElement,
        allElementsToDelete: MutableCollection<out PsiElement>,
        askUser: Boolean,
    ): Collection<PsiElement> = emptyList()

    /**
     * No extra conflicts beyond the usages themselves.
     *
     * Implemented explicitly rather than inherited: this became a default
     * method only in 2025.2, and it is still abstract on the 2024.2–2025.1
     * builds the plugin supports. Leaving it out compiles fine against the
     * newest platform and throws `AbstractMethodError` on the oldest, which is
     * exactly the class of problem the plugin verifier exists to catch.
     */
    override fun findConflicts(
        element: PsiElement,
        allElementsToDelete: Array<out PsiElement>,
    ): Collection<String>? = null

    override fun preprocessUsages(project: Project, usages: Array<out UsageInfo>): Array<UsageInfo> =
        @Suppress("UNCHECKED_CAST") (usages as Array<UsageInfo>)

    /** Nothing to rewrite before removal — the declaration is deleted whole. */
    override fun prepareForDeletion(element: PsiElement) = Unit

    override fun isToSearchInComments(element: PsiElement?): Boolean = searchInComments

    override fun setToSearchInComments(element: PsiElement?, enabled: Boolean) {
        searchInComments = enabled
    }

    override fun isToSearchForTextOccurrences(element: PsiElement?): Boolean = searchInText

    override fun setToSearchForTextOccurrences(element: PsiElement?, enabled: Boolean) {
        searchInText = enabled
    }

    private companion object {
        /**
         * The two dialog checkboxes, remembered for the session.
         *
         * Comments default ON and plain text OFF, matching Java: a name left
         * behind in a comment is worth seeing, while a substring match in
         * unrelated files is mostly noise.
         */
        var searchInComments = true
        var searchInText = false
    }
}
