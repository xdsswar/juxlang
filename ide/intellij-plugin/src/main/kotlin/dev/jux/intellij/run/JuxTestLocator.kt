package dev.jux.intellij.run

import com.intellij.execution.Location
import com.intellij.execution.PsiLocation
import com.intellij.execution.testframework.sm.runner.SMTestLocator
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiManager
import com.intellij.psi.search.FileTypeIndex
import com.intellij.psi.search.GlobalSearchScope
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.psi.JuxMethodDeclaration

/**
 * Resolves a test-tree node back to its `@Test` function — the target of the
 * `jux:test://pkg.fn` location hints [JuxTestEventsConverter] attaches. With
 * locations resolving, the SM runner provides double-click navigation and the
 * "Rerun Failed Tests" action for free.
 *
 * A test's display name is its package-qualified function name (§TS.2), so the
 * path splits at the LAST dot into (package, function); a bare name means a
 * file without a `package` statement. The walk scans the project's Jux files —
 * same idiom as [dev.jux.intellij.resolve.JuxTypeIndex].
 */
object JuxTestLocator : SMTestLocator {
    const val PROTOCOL = "jux:test"

    override fun getLocation(
        protocol: String,
        path: String,
        project: Project,
        scope: GlobalSearchScope,
    ): List<Location<*>> {
        if (protocol != PROTOCOL || path.isBlank()) return emptyList()
        // FileTypeIndex.getFiles throws IndexNotReadyException while indexing;
        // navigation re-resolves once smart mode returns.
        if (com.intellij.openapi.project.DumbService.isDumb(project)) return emptyList()
        val pkg = path.substringBeforeLast('.', "")
        val fn = path.substringAfterLast('.')

        val manager = PsiManager.getInstance(project)
        for (vf in FileTypeIndex.getFiles(JuxFileType, scope)) {
            val psi = manager.findFile(vf) ?: continue
            if (JuxTestDetector.packageName(psi) != pkg) continue
            for (decl in psi.children) {
                if (decl !is JuxMethodDeclaration) continue
                if (decl.name != fn) continue
                if (!JuxTestDetector.isTestOrHookFunction(decl)) continue
                return listOf(PsiLocation(decl.nameIdentifier ?: decl))
            }
        }
        return emptyList()
    }
}
