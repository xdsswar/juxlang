package dev.jux.intellij.resolve

import com.intellij.navigation.ChooseByNameContributorEx
import com.intellij.navigation.NavigationItem
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.util.Processor
import com.intellij.util.indexing.FindSymbolParameters
import com.intellij.util.indexing.IdFilter

/**
 * Go-to-Class (Ctrl+N) over the project's Jux type declarations.
 *
 * Backed by [JuxTypeIndex]'s on-demand PSI walk rather than a stub index —
 * fine at current project sizes; when a stub tree lands (plugin-gap.md B4)
 * only the index object changes, not these contributors.
 */
class JuxGotoClassContributor : ChooseByNameContributorEx {
    override fun processNames(processor: Processor<in String>, scope: GlobalSearchScope, filter: IdFilter?) {
        val project = scope.project ?: return
        JuxTypeIndex.forEachType(project, scope) { decl ->
            val name = decl.name ?: return@forEachType
            if (!processor.process(name)) return
        }
    }

    override fun processElementsWithName(
        name: String,
        processor: Processor<in NavigationItem>,
        parameters: FindSymbolParameters,
    ) {
        JuxTypeIndex.forEachType(parameters.project, parameters.searchScope) { decl ->
            if (decl.name == name && !processor.process(decl)) return
        }
    }
}

/**
 * Go-to-Symbol (Ctrl+Alt+Shift+N) over every Jux declaration worth jumping
 * to — types, methods, fields, constants, enum constants. Parameters and
 * locals are intentionally excluded (see [JuxTypeIndex.forEachSymbol]).
 */
class JuxGotoSymbolContributor : ChooseByNameContributorEx {
    override fun processNames(processor: Processor<in String>, scope: GlobalSearchScope, filter: IdFilter?) {
        val project = scope.project ?: return
        JuxTypeIndex.forEachSymbol(project, scope) { decl ->
            val name = decl.name ?: return@forEachSymbol
            if (!processor.process(name)) return
        }
    }

    override fun processElementsWithName(
        name: String,
        processor: Processor<in NavigationItem>,
        parameters: FindSymbolParameters,
    ) {
        JuxTypeIndex.forEachSymbol(parameters.project, parameters.searchScope) { decl ->
            if (decl.name == name && !processor.process(decl as NavigationItem) ) return
        }
    }
}
