package dev.jux.intellij.resolve

import com.intellij.openapi.application.QueryExecutorBase
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import com.intellij.psi.PsiReference
import com.intellij.psi.search.FileTypeIndex
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.LocalSearchScope
import com.intellij.psi.search.searches.ReferencesSearch
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.util.Processor
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Find Usages (and highlight usages) for an `operator` declaration.
 *
 * The platform's default search looks for the declaration's NAME as a word,
 * and an operator has only a symbol, so it would find nothing. This searcher
 * walks the Jux files in scope instead, takes every binary, range and
 * compound-assignment expression that uses the same symbol, and reports the
 * ones whose [JuxOperatorReference] resolves to the declaration: the same
 * operand-type choice the compiler makes, so `v * 2.0` counts for
 * `operator*(double)` and not for `operator*(Vec2)`.
 *
 * A file whose text does not contain the symbol is skipped before its tree is
 * walked.
 */
class JuxOperatorUsagesSearch :
    QueryExecutorBase<PsiReference, ReferencesSearch.SearchParameters>(true) {

    override fun processQuery(
        params: ReferencesSearch.SearchParameters,
        consumer: Processor<in PsiReference>,
    ) {
        val target = params.elementToSearch
        if (target.elementType !== E.OPERATOR_DECLARATION) return
        val symbol = JuxOperators.symbolOf(target) ?: return
        when (val scope = params.effectiveSearchScope) {
            is LocalSearchScope -> for (root in scope.scope) {
                if (!scan(root, symbol, target, consumer)) return
            }
            is GlobalSearchScope -> {
                val manager = PsiManager.getInstance(target.project)
                for (vf in FileTypeIndex.getFiles(JuxFileType, scope)) {
                    val file: PsiFile = manager.findFile(vf) ?: continue
                    if (!file.text.contains(symbol)) continue
                    if (!scan(file, symbol, target, consumer)) return
                }
            }
        }
    }

    /** Report every use under [root] that calls [target]; false when the consumer asked to stop. */
    private fun scan(root: PsiElement, symbol: String, target: PsiElement, consumer: Processor<in PsiReference>): Boolean {
        var keepGoing = true
        PsiTreeUtil.processElements(root) { e ->
            val t = e.elementType
            if (t === E.BINARY_EXPRESSION || t === E.RANGE_EXPRESSION || t === E.ASSIGNMENT_EXPRESSION) {
                val token = JuxOperators.operatorTokenOf(e)
                if (token != null && JuxOperators.symbolOfUse(token) == symbol) {
                    val ref = JuxOperatorReference.of(e)
                    if (ref != null && ref.resolve() == target && !consumer.process(ref)) keepGoing = false
                }
            }
            keepGoing
        }
        return keepGoing
    }
}
