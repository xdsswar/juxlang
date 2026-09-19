package dev.jux.intellij.resolve

import com.intellij.openapi.application.QueryExecutorBase
import com.intellij.psi.PsiElement
import com.intellij.psi.search.searches.DefinitionsScopedSearch
import com.intellij.util.Processor
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * Powers **Go To Implementation(s)** (Ctrl+Alt+B) and the implementations
 * popup for Jux: from a class/interface it yields the subtypes, from a method
 * the overriding/implementing methods — the keyboard counterpart of the
 * down-arrow gutter ([JuxSubtypeLineMarkerProvider]), reusing [JuxSubtypes].
 *
 * The platform hands us the resolved target (a declaration) or its name leaf;
 * we normalize to the enclosing [JuxTypeDeclaration] / [JuxMethodDeclaration]
 * and feed each result declaration to the consumer. A declaration's text
 * offset is its name, so navigation still lands there, and Quick Definition
 * (Ctrl+Shift+I) shows the whole declaration rather than one identifier.
 * Non-Jux / unrelated elements yield nothing.
 */
class JuxImplementationSearch :
    QueryExecutorBase<PsiElement, DefinitionsScopedSearch.SearchParameters>(/* requireReadAction = */ true) {

    override fun processQuery(
        params: DefinitionsScopedSearch.SearchParameters,
        consumer: Processor<in PsiElement>,
    ) {
        val element = params.element
        val type = element as? JuxTypeDeclaration ?: element.parent as? JuxTypeDeclaration
        if (type != null) {
            for (sub in JuxSubtypes.subtypesOf(type)) {
                if (!consumer.process(sub)) return
            }
            return
        }
        val method = element as? JuxMethodDeclaration ?: element.parent as? JuxMethodDeclaration
        if (method != null) {
            for (m in JuxSubtypes.overridingMethods(method)) {
                if (!consumer.process(m)) return
            }
        }
    }
}
