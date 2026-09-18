package dev.jux.intellij.project

import com.intellij.ide.util.PropertiesComponent
import com.intellij.notification.NotificationAction
import com.intellij.notification.NotificationGroupManager
import com.intellij.notification.NotificationType
import com.intellij.openapi.module.Module
import com.intellij.openapi.module.ModuleUtilCore
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.project.Project
import com.intellij.openapi.roots.ModuleRootManager
import com.intellij.openapi.roots.ModuleRootModificationUtil
import com.intellij.openapi.startup.ProjectActivity
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.search.FilenameIndex
import com.intellij.psi.search.GlobalSearchScope
import dev.jux.intellij.JuxPackageResolver
import org.jetbrains.jps.model.module.JpsModuleSourceRootType

/**
 * Finds the directories of a `jux.toml` project that should be Jux source
 * roots but aren't marked yet, and marks them (§I.4, §B.1 layout): `src/`
 * as a Jux Sources Root and `test/` as a Jux Test Sources Root.
 *
 * Marking is optional. With nothing marked the package resolver already
 * reads the manifest and gets every package right; the marks are what make
 * the Project view show the blue and green folders, scope tests as tests,
 * and let the language server be told the roots (§I.4).
 */
object JuxSourceRootDetector {

    /** One directory to mark, with the kind it should get. */
    data class Candidate(val module: Module, val dir: VirtualFile, val type: JuxSourceRootType)

    /**
     * The `src/` and `test/` directories of every `jux.toml` in the project
     * that a module contains and that are not already Jux roots. Requires a
     * read action and a ready index.
     */
    fun unmarkedRoots(project: Project): List<Candidate> {
        val out = ArrayList<Candidate>()
        val manifests = FilenameIndex.getVirtualFilesByName(JuxPackageResolver.MANIFEST, GlobalSearchScope.projectScope(project))
        for (manifest in manifests.sortedBy { it.path }) {
            val dir = manifest.parent ?: continue
            for ((name, type) in listOf("src" to JuxSourceRootType.SOURCE, "test" to JuxSourceRootType.TEST_SOURCE)) {
                val child = dir.findChild(name)?.takeIf { it.isDirectory } ?: continue
                val module = ModuleUtilCore.findModuleForFile(child, project) ?: continue
                if (rootTypeOf(module, child) is JuxSourceRootType) continue
                out += Candidate(module, child, type)
            }
        }
        return out
    }

    /**
     * Mark each candidate, replacing any other root kind the directory had
     * (a wizard that marked `src/` as a plain source folder, say). Runs its
     * own write action.
     */
    fun mark(candidates: List<Candidate>) {
        for ((module, group) in candidates.groupBy { it.module }) {
            ModuleRootModificationUtil.updateModel(module) { model ->
                for (candidate in group) {
                    val entry = model.contentEntries.firstOrNull { e ->
                        e.file?.let { com.intellij.openapi.vfs.VfsUtilCore.isAncestor(it, candidate.dir, false) } == true
                    } ?: continue
                    entry.sourceFolders.filter { it.file == candidate.dir }.forEach { entry.removeSourceFolder(it) }
                    entry.addSourceFolder(candidate.dir, candidate.type)
                }
            }
        }
    }

    /** The root kind [dir] is marked with in [module], or `null` when it is not a root. */
    fun rootTypeOf(module: Module, dir: VirtualFile): JpsModuleSourceRootType<*>? =
        ModuleRootManager.getInstance(module).contentEntries
            .flatMap { it.sourceFolders.asIterable() }
            .firstOrNull { it.file == dir }
            ?.rootType
}

/**
 * On opening a project with a `jux.toml`, offer once to mark its `src/` and
 * `test/` directories as Jux roots, in a non-modal balloon: **Mark**, or
 * **Don't Ask Again** for this project. The user can also mark by hand from
 * the Project view's **Mark Directory as** menu.
 */
class JuxSourceRootDetectorStartup : ProjectActivity {
    override suspend fun execute(project: Project) {
        // Tests drive [JuxSourceRootDetector] directly; a balloon there is noise.
        if (com.intellij.openapi.application.ApplicationManager.getApplication().isUnitTestMode) return
        if (PropertiesComponent.getInstance(project).getBoolean(DONT_ASK_KEY, false)) return
        DumbService.getInstance(project).runWhenSmart {
            val candidates = com.intellij.openapi.application.ReadAction.compute<List<JuxSourceRootDetector.Candidate>, RuntimeException> {
                if (project.isDisposed) emptyList() else JuxSourceRootDetector.unmarkedRoots(project)
            }
            if (candidates.isNotEmpty()) notify(project, candidates)
        }
    }

    private fun notify(project: Project, candidates: List<JuxSourceRootDetector.Candidate>) {
        val listed = candidates.joinToString(", ") { "${it.dir.parent?.name}/${it.dir.name}/" }
        NotificationGroupManager.getInstance()
            .getNotificationGroup(NOTIFICATION_GROUP)
            .createNotification(
                "Jux source roots",
                "This project has a jux.toml. Mark $listed as Jux source roots, so packages, tests and the Project view follow the Jux layout?",
                NotificationType.INFORMATION,
            )
            .addAction(NotificationAction.createSimpleExpiring("Mark") { JuxSourceRootDetector.mark(candidates) })
            .addAction(NotificationAction.createSimpleExpiring("Don't ask again") {
                PropertiesComponent.getInstance(project).setValue(DONT_ASK_KEY, true)
            })
            .notify(project)
    }

    companion object {
        /** The balloon group registered in `plugin.xml`. */
        const val NOTIFICATION_GROUP = "Jux Project"

        /** Per-project opt-out of the offer. */
        private const val DONT_ASK_KEY = "dev.jux.sourceRoots.dontAsk"
    }
}
