package dev.jux.intellij.project

import com.intellij.ide.util.projectWizard.ModuleWizardStep
import com.intellij.ide.util.projectWizard.ModuleBuilder
import com.intellij.ide.util.projectWizard.WizardContext
import com.intellij.openapi.Disposable
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.module.ModuleType
import com.intellij.openapi.roots.ModifiableRootModel
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VirtualFile

/** What the new project produces — drives the scaffold and the manifest. */
enum class JuxProjectKind { EXECUTABLE, LIBRARY }

/**
 * Scaffolds a new Jux project when the user picks the "Jux" generator in the
 * New Project / New Module wizard. A wizard step ([JuxProjectOptionsStep]) lets
 * the user choose what to build:
 *
 * - **Executable** → `src/main.jux` with a runnable `main()`; the default
 *   `[[bin]]` target builds a binary.
 * - **Library** → `src/lib.jux` exposing a public API; a `[lib]` target with
 *   the chosen `crate-type` (`lib` / `dylib` / `staticlib` / `cdylib`).
 *
 * ```text
 * <project>/
 * ├── jux.toml          # package manifest (§B.2) — [[bin]] or [lib]
 * ├── .gitignore
 * └── src/              # marked as a Jux Sources Root
 *     └── main.jux | lib.jux
 * ```
 *
 * All file I/O is wrapped so a failure can never crash the wizard.
 */
class JuxModuleBuilder : ModuleBuilder() {
    /** Chosen in the wizard step; defaults to a runnable executable. */
    var projectKind: JuxProjectKind = JuxProjectKind.EXECUTABLE
    /** `[lib] crate-type` when [projectKind] is LIBRARY. */
    var crateType: String = "lib"
    /** Whether to drop starter code into the entry file. */
    var generateSample: Boolean = true

    override fun getModuleType(): ModuleType<*> = JuxModuleType.instance
    override fun getPresentableName(): String = "Jux"
    override fun getDescription(): String =
        "Creates a Jux project: a jux.toml manifest and a src/ source root, as an executable or a library."
    override fun getGroupName(): String = "Jux"
    override fun getBuilderId(): String = "jux.module.builder"

    /** The extra wizard page where the user picks executable vs library. */
    override fun getCustomOptionsStep(context: WizardContext, parentDisposable: Disposable): ModuleWizardStep =
        JuxProjectOptionsStep(this)

    override fun setupRootModel(rootModel: ModifiableRootModel) {
        val contentEntry = doAddContentEntry(rootModel) ?: return
        val baseDir = contentEntry.file ?: return
        try {
            val pkg = JuxScaffold.packageNameFor(rootModel.module.name)
            writeChild(baseDir, "jux.toml", JuxScaffold.manifest(pkg, projectKind, crateType))
            writeChild(baseDir, ".gitignore", JuxScaffold.GITIGNORE)
            val src = VfsUtil.createDirectoryIfMissing(baseDir, "src")
            if (src != null) {
                writeChild(src, JuxScaffold.entryFileName(projectKind), JuxScaffold.entryContent(projectKind, generateSample))
                contentEntry.addSourceFolder(src, false)
            }
        } catch (e: Exception) {
            LOG.warn("Failed to scaffold Jux project", e)
        }
    }

    private fun writeChild(dir: VirtualFile, name: String, content: String) {
        val file = dir.findChild(name) ?: dir.createChildData(this, name)
        VfsUtil.saveText(file, content)
    }

    companion object {
        private val LOG = Logger.getInstance(JuxModuleBuilder::class.java)
    }
}
