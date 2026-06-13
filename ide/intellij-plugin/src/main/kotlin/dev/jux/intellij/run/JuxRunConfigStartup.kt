package dev.jux.intellij.run

import com.intellij.execution.RunManager
import com.intellij.execution.RunnerAndConfigurationSettings
import com.intellij.execution.configurations.ConfigurationTypeUtil
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.startup.ProjectActivity
import dev.jux.intellij.toolwindow.JuxToml
import java.io.File

/**
 * Makes the project's entry point **always** runnable from the toolbar — the
 * Run config for each binary module's `main` is auto-created on project open,
 * so the "Main" entry is in the run dropdown without the user first opening and
 * running the file. Driven by `jux.toml`: workspace members are walked, each
 * binary module's entry file is located (`src/main.jux`, else the first source
 * file with a `main`), and the config is named after its `[[bin]]` target.
 *
 * Idempotent and non-intrusive: deduped by the entry file path, so it never
 * duplicates a config the context producer ([JuxRunConfigurationProducer])
 * already made, and it only sets the *selected* config when none is selected.
 */
class JuxRunConfigStartup : ProjectActivity {
    private data class Entry(val name: String, val file: File)

    override suspend fun execute(project: Project) {
        if (ApplicationManager.getApplication().isUnitTestMode) return
        val base = project.basePath?.let(::File)?.takeIf { it.isDirectory } ?: return
        val entries = discoverEntries(base)
        if (entries.isEmpty()) return
        ApplicationManager.getApplication().invokeLater {
            if (!project.isDisposed) ensureConfigs(project, entries)
        }
    }

    /** One runnable entry per binary module (deduped by file). */
    private fun discoverEntries(base: File): List<Entry> {
        val out = LinkedHashMap<String, Entry>()
        for (dir in moduleDirs(base)) {
            val text = readOrEmpty(File(dir, "jux.toml"))
            val bins = JuxToml.bins(text)
            // Skip a lib-only module (a [lib] with no bin target and no main).
            if (JuxToml.hasLib(text) && bins.isEmpty() && !File(dir, "src/main.jux").isFile) continue
            val entry = entryFile(dir) ?: continue
            val name = bins.firstOrNull() ?: entry.nameWithoutExtension
            out.putIfAbsent(entry.path, Entry(name, entry))
        }
        return out.values.toList()
    }

    /** Module directories: workspace members, or the single root module. */
    private fun moduleDirs(base: File): List<File> {
        val text = readOrEmpty(File(base, "jux.toml"))
        val members = JuxToml.workspaceMembers(text)
        if (members.isEmpty()) {
            return if (File(base, "jux.toml").isFile) listOf(base) else emptyList()
        }
        val out = LinkedHashSet<File>()
        if (JuxToml.packageName(text) != null) out.add(base)
        for (m in members) {
            if (m.endsWith("/*")) {
                File(base, m.removeSuffix("/*")).listFiles()
                    ?.filter { it.isDirectory && File(it, "jux.toml").isFile }
                    ?.sortedBy { it.name }?.forEach(out::add)
            } else {
                File(base, m).takeIf { File(it, "jux.toml").isFile }?.let(out::add)
            }
        }
        return out.toList()
    }

    /** The module's entry `.jux`: `src/main.jux`, else the first file with a `main`. */
    private fun entryFile(moduleDir: File): File? {
        File(moduleDir, "src/main.jux").takeIf { it.isFile }?.let { return it }
        val src = File(moduleDir, "src").takeIf { it.isDirectory } ?: return null
        return src.walkTopDown().maxDepth(8)
            .filter { it.isFile && it.extension == "jux" }
            .firstOrNull { runCatching { JuxMainDetector.hasMain(it.readText()) }.getOrDefault(false) }
    }

    private fun ensureConfigs(project: Project, entries: List<Entry>) {
        val rm = RunManager.getInstance(project)
        val factory = ConfigurationTypeUtil
            .findConfigurationType(JuxRunConfigurationType::class.java)
            .configurationFactories[0]
        var firstNew: RunnerAndConfigurationSettings? = null
        for (e in entries) {
            val existing = rm.allSettings.firstOrNull {
                val c = it.configuration as? JuxRunConfiguration
                c != null && !c.isTestMode() && c.filePath == e.file.path
            }
            if (existing != null) {
                // A run from the gutter leaves a TEMPORARY config; promote it so
                // the entry point stays in the dropdown permanently.
                if (existing.isTemporary) existing.isTemporary = false
                continue
            }
            val settings = rm.createConfiguration(e.name, factory)
            (settings.configuration as JuxRunConfiguration).apply {
                mode = JuxRunConfiguration.MODE_RUN
                filePath = e.file.path
            }
            settings.isTemporary = false
            rm.addConfiguration(settings)
            if (firstNew == null) firstNew = settings
        }
        if (firstNew != null && rm.selectedConfiguration == null) {
            rm.selectedConfiguration = firstNew
        }
    }

    private fun readOrEmpty(f: File): String =
        runCatching { if (f.isFile) f.readText() else "" }.getOrDefault("")
}
