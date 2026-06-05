package dev.jux.intellij.actions

import com.intellij.ide.actions.CreateFileFromTemplateAction
import com.intellij.ide.actions.CreateFileFromTemplateDialog
import com.intellij.ide.fileTemplates.FileTemplate
import com.intellij.ide.fileTemplates.FileTemplateManager
import com.intellij.ide.fileTemplates.FileTemplateUtil
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDirectory
import com.intellij.psi.PsiFile
import dev.jux.intellij.JuxIcons
import dev.jux.intellij.JuxPackageResolver

/**
 * The single **New → Jux File** action.
 *
 * Its dialog offers the file kinds — plain File, Class, Interface, Enum,
 * Record, Annotation — each with its own icon, picking the matching internal
 * template (§I.5). This is the Kotlin-plugin idiom: one menu entry, a kind
 * picker, per-kind glyphs.
 *
 * Each created file gets its `package` line auto-filled from the target
 * directory's source-root-relative path via [JuxPackageResolver]; files
 * created outside any root omit the line (the template's `#if` guards it).
 */
class NewJuxFileAction :
    CreateFileFromTemplateAction("Jux File", "Create a new Jux file", JuxIcons.FILE) {

    override fun getActionName(directory: PsiDirectory?, newName: String, templateName: String?): String =
        "Create Jux File"

    override fun buildDialog(
        project: Project,
        directory: PsiDirectory,
        builder: CreateFileFromTemplateDialog.Builder,
    ) {
        builder
            .setTitle("New Jux File")
            .addKind("File", JuxIcons.NEW_FILE, "Jux File")
            .addKind("Class", JuxIcons.CLASS, "Jux Class")
            .addKind("Interface", JuxIcons.INTERFACE, "Jux Interface")
            .addKind("Enum", JuxIcons.ENUM, "Jux Enum")
            .addKind("Struct", JuxIcons.STRUCT, "Jux Struct")
            .addKind("Record", JuxIcons.RECORD, "Jux Record")
            .addKind("Annotation", JuxIcons.ANNOTATION, "Jux Annotation")
    }

    /**
     * Instantiate the chosen template with `NAME` and an inferred `PACKAGE`.
     *
     * We go through [FileTemplateUtil.createFromTemplate] directly so we can
     * seed the `PACKAGE` property — the platform's default path only knows how
     * to fill it for Java-aware file types.
     */
    override fun createFileFromTemplate(name: String, template: FileTemplate, dir: PsiDirectory): PsiFile? {
        return try {
            val project = dir.project
            val props = FileTemplateManager.getInstance(project).defaultProperties
            props.setProperty("NAME", name)
            props.setProperty(
                "PACKAGE",
                JuxPackageResolver.inferPackage(dir.virtualFile, project) ?: "",
            )
            FileTemplateUtil.createFromTemplate(template, name, props, dir).containingFile
        } catch (e: Exception) {
            LOG.error("Failed to create Jux file from template '${template.name}'", e)
            null
        }
    }

    companion object {
        private val LOG = Logger.getInstance(NewJuxFileAction::class.java)
    }
}
