package dev.jux.intellij.resolve

import com.intellij.openapi.project.Project
import com.intellij.psi.PsiManager
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.FileTypeIndex
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * A lightweight, project-wide index of Jux type declarations — the cross-file
 * resolution the per-file [JuxReference] lacks. Backed by the platform's
 * [FileTypeIndex] (every `.jux` file in the project) rather than a custom stub
 * index: type lookups are user-triggered and infrequent (override-methods,
 * future goto/completion), so on-demand PSI walking is fast enough and needs no
 * stub infrastructure.
 */
object JuxTypeIndex {
    /** The first top-level (or nested) type named [name] anywhere in the project. */
    fun findType(project: Project, name: String): JuxTypeDeclaration? {
        forEachType(project) { if (it.name == name) return it }
        return null
    }

    /** Bare names of every declared type in the project (for completion). */
    fun allTypeNames(project: Project): List<String> {
        val out = LinkedHashSet<String>()
        forEachType(project) { it.name?.let(out::add) }
        return out.toList()
    }

    private inline fun forEachType(project: Project, action: (JuxTypeDeclaration) -> Unit) {
        val scope = GlobalSearchScope.allScope(project)
        val manager = PsiManager.getInstance(project)
        for (vf in FileTypeIndex.getFiles(JuxFileType, scope)) {
            val psi = manager.findFile(vf) ?: continue
            for (decl in PsiTreeUtil.findChildrenOfType(psi, JuxTypeDeclaration::class.java)) {
                action(decl)
            }
        }
    }
}
