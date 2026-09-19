package dev.jux.intellij.project

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.execution.process.CapturingProcessHandler
import com.intellij.ide.util.projectWizard.ModuleWizardStep
import com.intellij.ide.util.projectWizard.ModuleBuilder
import com.intellij.ide.util.projectWizard.WizardContext
import com.intellij.openapi.Disposable
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.module.ModuleType
import com.intellij.openapi.roots.ModifiableRootModel
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VirtualFile
import dev.jux.intellij.run.JuxToolchain
import java.io.File
import java.nio.file.Files

/**
 * What the new project produces, the three templates of `jux new`
 * (JUX-BUILD-SYSTEM-ADDENDUM §B.15.1): a binary, a library (`--lib`), or a
 * workspace root (`--workspace`).
 */
enum class JuxProjectKind { EXECUTABLE, LIBRARY, WORKSPACE }

/**
 * Scaffolds a new Jux project when the user picks the "Jux" generator in the
 * New Project / New Module wizard. A wizard step ([JuxProjectOptionsStep]) lets
 * the user choose what to build:
 *
 * - **Binary** -> `src/main.jux` with a runnable `main()`.
 * - **Library** -> a package-less `src/lib.jux` crate root, the code in its
 *   package directory, and a first test; `[lib]` with the chosen `crate-type`
 *   (`lib` / `dylib` / `staticlib` / `cdylib`).
 * - **Workspace** -> a root `jux.toml` with an empty `members` list and the
 *   `[workspace.package]` / `[workspace.dependencies]` tables members inherit
 *   from.
 *
 * The files are `jux new`'s own: when a `jux` toolchain is installed the
 * builder runs `jux new` in a scratch directory and copies what it wrote, so
 * the IDE and the command line always agree; without one it writes the same
 * templates from [JuxScaffold.files]. All file I/O is wrapped so a failure can
 * never crash the wizard.
 */
class JuxModuleBuilder : ModuleBuilder() {
    /** Chosen in the wizard step; defaults to a runnable binary. */
    var projectKind: JuxProjectKind = JuxProjectKind.EXECUTABLE
    /** `[lib] crate-type` when [projectKind] is LIBRARY. */
    var crateType: String = "lib"
    /** Whether to drop starter code into the entry file. */
    var generateSample: Boolean = true

    override fun getModuleType(): ModuleType<*> = JuxModuleType.instance
    override fun getPresentableName(): String = "Jux"
    override fun getDescription(): String =
        "Creates a Jux project with jux new: a binary, a library or a workspace."
    override fun getGroupName(): String = "Jux"
    override fun getBuilderId(): String = "jux.module.builder"

    /** The extra wizard page where the user picks binary, library or workspace. */
    override fun getCustomOptionsStep(context: WizardContext, parentDisposable: Disposable): ModuleWizardStep =
        JuxProjectOptionsStep(this)

    override fun setupRootModel(rootModel: ModifiableRootModel) {
        val contentEntry = doAddContentEntry(rootModel) ?: return
        val baseDir = contentEntry.file ?: return
        try {
            val files = templateFiles(baseDir.name)
            for ((rel, content) in files) {
                val parent = rel.substringBeforeLast('/', "")
                val dir = if (parent.isEmpty()) baseDir else VfsUtil.createDirectoryIfMissing(baseDir, parent) ?: continue
                writeChild(dir, rel.substringAfterLast('/'), content)
            }
            // Jux Sources Roots, not plain source folders: packages are read
            // from them (§I.4) and the Project view shows them as Jux's.
            baseDir.findChild("src")?.let { contentEntry.addSourceFolder(it, JuxSourceRootType.SOURCE) }
            baseDir.findChild("test")?.let { contentEntry.addSourceFolder(it, JuxSourceRootType.TEST_SOURCE) }
        } catch (e: Exception) {
            LOG.warn("Failed to scaffold Jux project", e)
        }
    }

    /**
     * The project's files: what `jux new` writes for [dirName] and the chosen
     * kind, adjusted for the wizard's crate-type and sample choices, else the
     * same templates written by the IDE.
     */
    private fun templateFiles(dirName: String): Map<String, String> {
        val fallback = JuxScaffold.files(dirName, projectKind, crateType, generateSample)
        val fromCli = runJuxNew(dirName) ?: return fallback
        val out = LinkedHashMap(fromCli)
        if (projectKind == JuxProjectKind.LIBRARY && crateType != "lib") {
            out["jux.toml"]?.let { out["jux.toml"] = it.replaceFirst("[lib]\n", "[lib]\ncrate-type = [\"$crateType\"]\n") }
        }
        if (!generateSample && projectKind == JuxProjectKind.EXECUTABLE) {
            out["src/main.jux"] = JuxScaffold.entryContent(projectKind, sample = false)
        }
        return out
    }

    /** `jux new [--lib | --workspace] <dirName>` in a scratch directory; its files, or null. */
    private fun runJuxNew(dirName: String): Map<String, String>? {
        val jux = JuxToolchain.find("jux") ?: return null
        val scratch = try {
            Files.createTempDirectory("jux-new").toFile()
        } catch (_: Exception) {
            return null
        }
        return try {
            val args = buildList {
                add("new")
                when (projectKind) {
                    JuxProjectKind.LIBRARY -> add("--lib")
                    JuxProjectKind.WORKSPACE -> add("--workspace")
                    JuxProjectKind.EXECUTABLE -> {}
                }
                add(dirName)
            }
            val out = CapturingProcessHandler(
                GeneralCommandLine(jux).withParameters(args).withWorkDirectory(scratch).withCharset(Charsets.UTF_8),
            ).runProcess(20_000)
            val made = File(scratch, dirName)
            if (out.exitCode != 0 || !made.isDirectory) return null
            made.walkTopDown().filter { it.isFile }
                .associate { it.relativeTo(made).invariantSeparatorsPath to it.readText() }
                .takeIf { it.containsKey("jux.toml") }
        } catch (e: Exception) {
            LOG.info("jux new failed; writing the built-in template", e)
            null
        } finally {
            scratch.deleteRecursively()
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
