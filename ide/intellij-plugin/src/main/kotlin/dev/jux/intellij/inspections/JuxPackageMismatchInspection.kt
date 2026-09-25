package dev.jux.intellij.inspections

import com.intellij.codeInsight.intention.preview.IntentionPreviewInfo
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.elementType
import dev.jux.intellij.JuxPackageResolver
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile

/**
 * A file's `package` line must name the package its location implies (§I.4,
 * §B.1.1): `src/com/example/Foo.jux` declares `package com.example;`, and a
 * file directly in `src/` declares none. `juxc` rejects the mismatch too
 * (`E0301`), so this is the same rule, said while the file is being edited.
 *
 * Only a location the Jux layout vouches for is checked: a marked Jux root or
 * a `jux.toml` project ([JuxPackageResolver.expectedPackages]). A loose file,
 * or one under a root the plugin merely guessed, takes its package from its
 * declaration and is left alone. A test file may also add `.test` to its
 * production package, the §B.1.2 convention, and an entry file a `[[bin]] path`
 * names is exempt from the requirement entirely (ERRATA E103): `src/bin/server.jux`
 * is a program's entry point rather than a member of a package `bin`.
 *
 * Fixes, as Java offers them:
 *  - **Set package name to `expected`**: rewrite (or remove) the `package` line.
 *  - **Move file to the declared package's directory**: move the file to the directory its
 *    declaration names, under the same root. The package does not change, so
 *    no import anywhere needs touching.
 *  - **Add `package expected;`** when the line is missing.
 */
class JuxPackageMismatchInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val vFile = file.originalFile.virtualFile ?: return null
        val accepted = JuxPackageResolver.expectedPackages(vFile, file.project) ?: return null
        val expected = accepted.first()
        val statement = packageStatement(file)
        val declared = JuxAutoImport.packageOfFile(file)
        if (declared in accepted) return null

        val problem = when {
            // An entry file a `[[bin]] path` names is not a member of the
            // package tree (§B.1.1, ERRATA E103), so `src/bin/server.jux` owes
            // no `package bin;` -- a name no `import` could usefully reach,
            // since the manifest key is how the file is found. Only the
            // REQUIREMENT is lifted: a package the file does declare falls
            // through to the mismatch branch below and is still checked against
            // its directory.
            statement == null && JuxPackageResolver.isBinEntryFile(vFile, file.project) -> return null
            statement == null -> manager.createProblemDescriptor(
                firstSignificant(file) ?: return null,
                "Missing package declaration: this file's location puts it in `$expected`",
                isOnTheFly,
                arrayOf<LocalQuickFix>(AddPackageFix(expected)),
                ProblemHighlightType.GENERIC_ERROR_OR_WARNING,
            )
            else -> {
                val nameElement = statement.node.findChildByType(E.QUALIFIED_NAME)?.psi ?: statement
                val message = if (expected.isEmpty()) {
                    "A file directly in the source root has no package; `$declared` does not correspond to the file location"
                } else {
                    "Package name `$declared` does not correspond to the file location. Expected `$expected`"
                }
                val fixes = ArrayList<LocalQuickFix>()
                fixes += SetPackageFix(expected)
                if (canMoveTo(file, declared)) fixes += MoveFileToPackageFix(declared)
                manager.createProblemDescriptor(
                    nameElement,
                    message,
                    isOnTheFly,
                    fixes.toTypedArray(),
                    ProblemHighlightType.GENERIC_ERROR_OR_WARNING,
                )
            }
        }
        return arrayOf(problem)
    }

    /**
     * True when the file could move to [declared]'s directory: it has a root
     * the layout vouches for, and no file of the same name is already there.
     */
    private fun canMoveTo(file: PsiFile, declared: String): Boolean {
        val vFile = file.originalFile.virtualFile ?: return false
        val root = JuxPackageResolver.rootFor(vFile, file.project)?.takeIf { it.authoritative } ?: return false
        val existing = JuxPackageResolver.directoryFor(root, declared, create = false) ?: return true
        return existing.findChild(vFile.name) == null
    }

    companion object {
        /** The file's `package` statement, when it has one. */
        fun packageStatement(file: PsiFile): PsiElement? =
            file.children.firstOrNull { it.elementType === E.PACKAGE_STATEMENT }

        /** The first thing in the file that is not whitespace or a comment. */
        fun firstSignificant(file: PsiFile): PsiElement? =
            file.children.firstOrNull { it !is PsiWhiteSpace && it !is PsiComment && it.textLength > 0 }
    }
}

/** Rewrite the `package` line to [expected], or remove it when [expected] is the root. */
class SetPackageFix(private val expected: String) : LocalQuickFix {
    override fun getFamilyName(): String = "Set package name to match the file location"

    override fun getName(): String =
        if (expected.isEmpty()) "Remove package declaration" else "Set package name to `$expected`"

    override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
        val file = descriptor.psiElement?.containingFile ?: return
        val statement = JuxPackageMismatchInspection.packageStatement(file) ?: return
        val document = PsiDocumentManager.getInstance(project).getDocument(file) ?: return
        if (expected.isEmpty()) {
            // The statement, and the blank lines after it, go.
            val text = document.charsSequence
            var end = statement.textRange.endOffset
            while (end < text.length && (text[end] == '\n' || text[end] == '\r' || text[end] == ' ' || text[end] == '\t')) end++
            document.deleteString(statement.textRange.startOffset, end)
        } else {
            val name = statement.node.findChildByType(E.QUALIFIED_NAME)?.textRange ?: return
            document.replaceString(name.startOffset, name.endOffset, expected)
        }
        PsiDocumentManager.getInstance(project).commitDocument(document)
    }
}

/** Insert `package expected;` above the file's first declaration. */
class AddPackageFix(private val expected: String) : LocalQuickFix {
    override fun getFamilyName(): String = "Add package declaration"

    override fun getName(): String = "Add `package $expected;`"

    override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
        val file = descriptor.psiElement?.containingFile ?: return
        val document = PsiDocumentManager.getInstance(project).getDocument(file) ?: return
        // After a leading header comment, if there is one, as Java does.
        val offset = JuxPackageMismatchInspection.firstSignificant(file)?.textRange?.startOffset ?: 0
        document.insertString(offset, "package $expected;\n\n")
        PsiDocumentManager.getInstance(project).commitDocument(document)
    }
}

/**
 * Move the file to the directory [declared] names, under its current root,
 * creating the directories as needed. Its package is unchanged, so nothing
 * that imports from it is affected.
 */
class MoveFileToPackageFix(private val declared: String) : LocalQuickFix {
    override fun getFamilyName(): String = "Move file to the directory of its package"

    override fun getName(): String =
        if (declared.isEmpty()) "Move file to the source root" else "Move file to `${declared.replace('.', '/')}/`"

    override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
        val file = descriptor.psiElement?.containingFile ?: return
        val vFile = file.virtualFile ?: return
        val root = JuxPackageResolver.rootFor(vFile, project)?.takeIf { it.authoritative } ?: return
        PsiDocumentManager.getInstance(project).getDocument(file)?.let {
            PsiDocumentManager.getInstance(project).doPostponedOperationsAndUnblockDocument(it)
            com.intellij.openapi.fileEditor.FileDocumentManager.getInstance().saveDocument(it)
        }
        val target = JuxPackageResolver.directoryFor(root, declared, create = true) ?: return
        if (target.findChild(vFile.name) != null) return
        vFile.move(this, target)
    }

    /** A file move has nothing to show as a text diff. */
    override fun generatePreview(project: Project, previewDescriptor: ProblemDescriptor): IntentionPreviewInfo =
        IntentionPreviewInfo.Html("Moves the file to <code>${declared.replace('.', '/').ifEmpty { "(source root)" }}/</code>.")
}
