package dev.jux.intellij.project

import com.intellij.ide.util.projectWizard.ModuleBuilder
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.module.ModuleType
import com.intellij.openapi.roots.ModifiableRootModel
import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VirtualFile

/**
 * Scaffolds a new Jux project when the user picks the "Jux" generator in the
 * New Project / New Module wizard. It creates:
 *
 * ```text
 * <project>/
 * ├── jux.toml          # package manifest (§B.2)
 * ├── .gitignore
 * └── src/              # marked as a Jux Sources Root
 *     └── main.jux      # a runnable hello-world
 * ```
 *
 * All file I/O is wrapped so a failure can never crash the wizard.
 */
class JuxModuleBuilder : ModuleBuilder() {
    override fun getModuleType(): ModuleType<*> = JuxModuleType.instance
    override fun getPresentableName(): String = "Jux"
    override fun getDescription(): String =
        "Creates a Jux project: a jux.toml manifest and a src/ source root with a starter main.jux."
    override fun getGroupName(): String = "Jux"
    override fun getBuilderId(): String = "jux.module.builder"

    override fun setupRootModel(rootModel: ModifiableRootModel) {
        val contentEntry = doAddContentEntry(rootModel) ?: return
        val baseDir = contentEntry.file ?: return
        try {
            val pkg = packageNameFor(rootModel.module.name)
            writeChild(baseDir, "jux.toml", juxToml(pkg))
            writeChild(baseDir, ".gitignore", GITIGNORE)
            val src = VfsUtil.createDirectoryIfMissing(baseDir, "src")
            if (src != null) {
                writeChild(src, "main.jux", MAIN_JUX)
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

    /**
     * Derive a valid reverse-DNS `package.name` (§B.2 regex) from the module
     * name: lowercase, non-alphanumerics → `_`, leading digits/underscores
     * trimmed, prefixed under `com.example.` so it always has ≥2 segments.
     */
    private fun packageNameFor(moduleName: String): String {
        val cleaned = moduleName.lowercase().map { if (it.isLetterOrDigit()) it else '_' }.joinToString("")
        val safe = cleaned.trimStart('_', '0', '1', '2', '3', '4', '5', '6', '7', '8', '9')
            .ifEmpty { "app" }
        return "com.example.$safe"
    }

    private fun juxToml(pkg: String): String =
        """
        [package]
        name = "$pkg"
        version = "0.1.0"
        edition = "2026"

        [dependencies]
        """.trimIndent() + "\n"

    companion object {
        private val LOG = Logger.getInstance(JuxModuleBuilder::class.java)

        private val MAIN_JUX =
            """
            public void main() {
                print("Hello, Jux!");
            }
            """.trimIndent() + "\n"

        private const val GITIGNORE = "/target/\n*.exe\n"
    }
}
