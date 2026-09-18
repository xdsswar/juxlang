package dev.jux.intellij.refactoring

import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiDirectory
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import com.intellij.psi.PsiReference
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.searches.ReferencesSearch
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.refactoring.move.MoveCallback
import com.intellij.refactoring.move.MoveHandlerDelegate
import dev.jux.intellij.JuxPackageResolver
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * Move Class (`F6` on a top-level type): the type moves to another package.
 *
 * Per spec §3.1 a type lives in `<Name>.jux` in its package's directory, so
 * the move is a file move: the whole file when the type is all it declares,
 * otherwise the type is split out into a new `<Name>.jux` there. Then:
 *
 * - the moved file's `package` line names the new package, and it imports the
 *   types of its old package it used without an import before;
 * - every file that used the type is fixed: an `import old.Name` becomes
 *   `import new.Name` (or goes, in the new package), a grouped import loses
 *   the name and a plain import is added, a file that saw it through its own
 *   package or a wildcard gets an import, and a qualified `old.Name` is
 *   rewritten.
 */
class JuxMoveClass(val type: JuxTypeDeclaration, val targetPackage: String) {

    /** Problems that stop the move; empty when it can run. */
    fun problems(): List<String> {
        val out = ArrayList<String>()
        val name = type.name ?: return listOf("The type has no name.")
        if (type.parent !is JuxFile) out += "Only a top-level type can be moved to another package."
        if (targetPackage.isNotEmpty() && !targetPackage.split('.').all { JuxRefactoringInput.identifierProblem(it) == null }) {
            out += "`$targetPackage` is not a valid package name."
        }
        if (targetPackage == JuxAutoImport.packageOfFile(type.containingFile)) out += "`$name` is already in that package."
        return out
    }

    /** Run the move; opens its own write command. */
    fun run(project: Project) {
        val name = type.name ?: return
        val file = type.containingFile as JuxFile
        val oldPackage = JuxAutoImport.packageOfFile(file)
        val splitOut = file.children.count { it is JuxTypeDeclaration || it.elementType === E.METHOD_DECLARATION } > 1

        // Everything is worked out on the unchanged tree, then written.
        val references = ReferencesSearch.search(type, GlobalSearchScope.projectScope(project)).findAll()
        val otherFiles = references.map { it.element.containingFile }.filter { it !== file }.distinct()
        val edits = ArrayList<JuxRefactoringUtil.Edit>()
        for (g in otherFiles) edits += referencingFileEdits(g, references, oldPackage, name)
        val needed = importsNeededByMovedType(oldPackage, file)

        WriteCommandAction.writeCommandAction(project).withName(TITLE).run<Exception> {
            JuxRefactoringUtil.applyEdits(project, edits)
            val targetDir = targetDirectory(project, file) ?: throw IllegalStateException("No source root for ${file.name}")
            if (!splitOut) {
                JuxRefactoringUtil.applyEdits(project, movedFileEdits(file, oldPackage, needed), reformat = false)
                file.virtualFile.move(this, targetDir)
            } else {
                // The type goes to its own file; what stays behind still sees it.
                val text = newFileText(file, needed)
                val stays = file.children.any { child ->
                    child !== type && references.any { r -> r.element.containingFile === file && child.textRange.contains(r.element.textRange) }
                }
                val staying = ArrayList<JuxRefactoringUtil.Edit>()
                staying += JuxRefactoringUtil.Edit(file, JuxRefactoringUtil.memberDeletionRange(type), "")
                if (stays && targetPackage.isNotEmpty()) importEdit(file, "$targetPackage.$name")?.let { staying += it }
                JuxRefactoringUtil.applyEdits(project, staying)
                val dir = PsiManager.getInstance(project).findDirectory(targetDir)!!
                val created = dir.createFile("$name.jux")
                PsiDocumentManager.getInstance(project).getDocument(created)?.let {
                    it.setText(text)
                    PsiDocumentManager.getInstance(project).commitDocument(it)
                }
            }
        }
    }

    // ---- the moved file ------------------------------------------------------

    /** Its `package` line rewritten and the imports it now needs added. */
    private fun movedFileEdits(file: PsiFile, oldPackage: String, needed: List<String>): List<JuxRefactoringUtil.Edit> {
        val out = ArrayList<JuxRefactoringUtil.Edit>()
        val pkg = file.node.findChildByType(E.PACKAGE_STATEMENT)?.psi
        val imports = file.children.filter { it.elementType === E.IMPORT_STATEMENT }
        // Imports of the new package's own types are now redundant.
        for (imp in imports) {
            if (importedPackage(imp) == targetPackage && !imp.text.contains('{') && !imp.text.contains('*')) {
                out += JuxRefactoringUtil.Edit(file, lineRange(imp), "")
            }
        }
        val added = needed.joinToString("") { "import $it;\n" }
        when {
            pkg == null && targetPackage.isNotEmpty() ->
                out += JuxRefactoringUtil.Edit(file, TextRange.from(0, 0), "package $targetPackage;\n\n$added" + if (added.isNotEmpty() && imports.isEmpty()) "\n" else "")
            pkg != null && targetPackage.isEmpty() -> {
                out += JuxRefactoringUtil.Edit(file, lineRange(pkg, swallowBlank = true), "")
                if (added.isNotEmpty()) out += JuxRefactoringUtil.Edit(file, TextRange.from(imports.firstOrNull()?.textRange?.startOffset ?: 0, 0), added)
            }
            pkg != null -> {
                pkg.node.findChildByType(E.QUALIFIED_NAME)?.psi?.let { out += JuxRefactoringUtil.Edit(file, it.textRange, targetPackage) }
                if (added.isNotEmpty()) {
                    val anchor = imports.lastOrNull()
                    out += if (anchor != null) {
                        JuxRefactoringUtil.Edit(file, TextRange.from(anchor.textRange.endOffset, 0), "\n" + added.trimEnd())
                    } else {
                        JuxRefactoringUtil.Edit(file, TextRange.from(pkg.textRange.endOffset, 0), "\n\n" + added.trimEnd())
                    }
                }
            }
        }
        return out
    }

    /** The text of the new file a split-out type goes to. */
    private fun newFileText(file: PsiFile, needed: List<String>): String {
        val header = if (targetPackage.isEmpty()) "" else "package $targetPackage;\n\n"
        val imports = file.children.filter { it.elementType === E.IMPORT_STATEMENT && importedPackage(it) != targetPackage }
            .map { it.text.trim() } + needed.map { "import $it;" }
        val importBlock = if (imports.isEmpty()) "" else imports.distinct().joinToString("\n", postfix = "\n\n")
        return header + importBlock + type.text.trim() + "\n"
    }

    /**
     * `old.X` for each type of the old package the moved type uses without an
     * import: once it leaves the package, it no longer sees them for free. The
     * root package needs none (it is visible everywhere).
     */
    private fun importsNeededByMovedType(oldPackage: String, file: PsiFile): List<String> {
        if (oldPackage.isEmpty()) return emptyList()
        val out = LinkedHashSet<String>()
        val refs = PsiTreeUtil.collectElements(type) { it.elementType === E.TYPE_REFERENCE || it.elementType === E.REFERENCE_EXPRESSION }
        for (ref in refs) {
            val simple = ref.text.trim().substringBefore('<').takeIf { it.isNotEmpty() && '.' !in it } ?: continue
            if (!simple[0].isUpperCase()) continue
            val target = JuxTypeEngine.resolveTypeName(ref, simple) as? JuxTypeDeclaration ?: continue
            if (target === type || PsiTreeUtil.isAncestor(type, target, false)) continue
            if (target.parent !is JuxFile) continue
            // A type of the old package stays there (a file-mate of a split-out
            // type included); only the moved type itself goes.
            if (JuxAutoImport.packageOf(target) == oldPackage && oldPackage != targetPackage) {
                out += "$oldPackage.${target.name}"
            }
        }
        return out.toList()
    }

    // ---- files that use the type --------------------------------------------

    private fun referencingFileEdits(
        g: PsiFile,
        references: Collection<PsiReference>,
        oldPackage: String,
        name: String,
    ): List<JuxRefactoringUtil.Edit> {
        val out = ArrayList<JuxRefactoringUtil.Edit>()
        val gPackage = JuxAutoImport.packageOfFile(g)
        val oldFqn = if (oldPackage.isEmpty()) name else "$oldPackage.$name"
        val newFqn = if (targetPackage.isEmpty()) name else "$targetPackage.$name"
        var covered = false
        for (imp in g.children.filter { it.elementType === E.IMPORT_STATEMENT }) {
            val flat = imp.text.replace(Regex("\\s"), "")
            when {
                // `import old.Name;` / `import old.Name as N;`
                flat == "import$oldFqn;" || flat.startsWith("import${oldFqn}as") -> {
                    covered = true
                    if (gPackage == targetPackage && !flat.contains("as")) {
                        out += JuxRefactoringUtil.Edit(g, lineRange(imp), "")
                    } else {
                        imp.node.findChildByType(E.QUALIFIED_NAME)?.psi?.let { out += JuxRefactoringUtil.Edit(g, it.textRange, newFqn) }
                    }
                }
                // `import old.{A, Name, B};`
                flat.startsWith("import$oldPackage.{") && Regex("[{,]$name[,}]").containsMatchIn(flat) -> {
                    val items = flat.substringAfter('{').substringBefore('}').split(',').filter { it.isNotEmpty() && it != name }
                    val replacement = if (items.isEmpty()) "" else "import $oldPackage.{${items.joinToString(", ")}};"
                    out += JuxRefactoringUtil.Edit(g, if (items.isEmpty()) lineRange(imp) else imp.textRange, replacement)
                }
            }
        }
        // Qualified uses: `old.Name` in code.
        if (oldPackage.isNotEmpty()) {
            for (ref in references) {
                val el = ref.element
                if (el.containingFile !== g) continue
                if (el.elementType === E.TYPE_REFERENCE && el.text.trim().substringBefore('<') == oldFqn) {
                    val start = el.textRange.startOffset
                    out += JuxRefactoringUtil.Edit(g, TextRange.from(start, oldFqn.length), newFqn)
                }
            }
        }
        // It saw the type through its own package or a wildcard: now it needs an import.
        if (!covered && targetPackage.isNotEmpty() && gPackage != targetPackage) {
            val viaWildcard = g.children.any { it.elementType === E.IMPORT_STATEMENT && it.text.replace(Regex("\\s"), "") == "import$oldPackage.*;" }
            val usesBare = references.any { r -> r.element.containingFile === g && r.element.text.trim().substringBefore('<') == name }
            if (usesBare && (gPackage == oldPackage || viaWildcard)) importEdit(g, newFqn)?.let { out += it }
        }
        return out
    }

    // ---- helpers -------------------------------------------------------------

    private fun importedPackage(imp: PsiElement): String? =
        imp.node.findChildByType(E.QUALIFIED_NAME)?.text?.replace(Regex("\\s"), "")?.substringBeforeLast('.', "")

    /** An `import fqn;` after the last import (or the package line). */
    private fun importEdit(file: PsiFile, fqn: String): JuxRefactoringUtil.Edit? {
        val anchor = file.children.lastOrNull { it.elementType === E.IMPORT_STATEMENT }
            ?: file.node.findChildByType(E.PACKAGE_STATEMENT)?.psi
        return if (anchor != null) {
            val gap = if (anchor.elementType === E.PACKAGE_STATEMENT) "\n\n" else "\n"
            JuxRefactoringUtil.Edit(file, TextRange.from(anchor.textRange.endOffset, 0), "${gap}import $fqn;")
        } else {
            JuxRefactoringUtil.Edit(file, TextRange.from(0, 0), "import $fqn;\n\n")
        }
    }

    /** [element]'s whole line, newline included (and a blank line after it, when asked). */
    private fun lineRange(element: PsiElement, swallowBlank: Boolean = false): TextRange {
        val text = element.containingFile.text
        var s = element.textRange.startOffset
        while (s > 0 && text[s - 1] != '\n' && text[s - 1].isWhitespace()) s--
        var e = element.textRange.endOffset
        while (e < text.length && text[e] != '\n' && text[e].isWhitespace()) e++
        if (e < text.length && text[e] == '\n') e++
        if (swallowBlank) while (e < text.length && text[e] == '\n') e++
        return TextRange(s, e)
    }

    /**
     * The directory of [targetPackage] under the source root of [file],
     * created when missing.
     */
    /**
     * The target package's directory, under the same package root the file is
     * in now ([JuxPackageResolver], §I.4). With no root at all (a loose file),
     * the package directories are made next to the file.
     */
    private fun targetDirectory(project: Project, file: PsiFile): VirtualFile? {
        val vf = file.virtualFile ?: return null
        val root = JuxPackageResolver.rootFor(vf, project)
            ?: JuxPackageResolver.Root(vf.parent ?: return null, JuxPackageResolver.Kind.FALLBACK)
        return JuxPackageResolver.directoryFor(root, targetPackage, create = true)
    }

    companion object {
        const val TITLE = "Move Class"
    }
}

/**
 * F6 (and drag-and-drop in the Project view) on a Jux top-level type: asks for
 * the target package and runs [JuxMoveClass].
 */
class JuxMoveClassHandler : MoveHandlerDelegate() {

    override fun canMove(elements: Array<out PsiElement>, targetContainer: PsiElement?, reference: PsiReference?): Boolean =
        elements.size == 1 && (elements[0] as? JuxTypeDeclaration)?.parent is JuxFile

    override fun isValidTarget(targetElement: PsiElement?, sources: Array<out PsiElement>): Boolean =
        targetElement is PsiDirectory

    override fun tryToMove(element: PsiElement, project: Project, dataContext: DataContext?, reference: PsiReference?, editor: Editor?): Boolean {
        val type = (element as? JuxTypeDeclaration) ?: (element.parent as? JuxTypeDeclaration) ?: return false
        if (type.parent !is JuxFile) return false
        move(project, type, null, editor)
        return true
    }

    override fun doMove(project: Project, elements: Array<out PsiElement>, targetContainer: PsiElement?, callback: MoveCallback?) {
        val type = elements.firstOrNull() as? JuxTypeDeclaration ?: return
        move(project, type, targetContainer as? PsiDirectory, null)
        callback?.refactoringCompleted()
    }

    private fun move(project: Project, type: JuxTypeDeclaration, directory: PsiDirectory?, editor: Editor?) {
        val current = JuxAutoImport.packageOfFile(type.containingFile)
        val target = directory?.virtualFile?.let { JuxPackageResolver.inferPackage(it, project) }
            ?: JuxRefactoringInput.ask(project, JuxMoveClass.TITLE, "Move `${type.name}` to package:", current) { pkg ->
                JuxMoveClass(type, pkg).problems().firstOrNull()
            }
            ?: return
        val move = JuxMoveClass(type, target)
        move.problems().firstOrNull()?.let {
            JuxRefactoringInput.refuse(project, editor, JuxMoveClass.TITLE, it)
            return
        }
        move.run(project)
    }
}
