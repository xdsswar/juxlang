package dev.jux.intellij.intentions

import com.intellij.codeInsight.intention.HighPriorityAction
import com.intellij.codeInsight.intention.IntentionAction
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.ui.popup.JBPopupFactory
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * "Import 'pkg.Type'" — the Alt+Enter action that adds the missing `import`
 * for an unqualified type name that names a project type living in another
 * package. The IDE-side counterpart of completion's auto-import
 * ([JuxAutoImport]); it's what turns a red, unresolved cross-package type into
 * compilable code without hand-writing the import.
 *
 * Offered only when it's unambiguously useful: the caret is on a bare (no `.`)
 * [E.TYPE_REFERENCE], the name isn't a type declared in this file, isn't
 * already imported, and at least one project type with that name exists in a
 * *different, named* package. A single candidate imports directly; several
 * (same simple name in different packages) pop a chooser.
 */
class JuxImportTypeIntention : IntentionAction, HighPriorityAction {
    // Computed in isAvailable (runs first), consumed by getText/invoke.
    private var cachedName: String? = null
    private var cachedFqns: List<String> = emptyList()

    override fun getText(): String =
        cachedFqns.singleOrNull()?.let { "Import '$it'" } ?: "Import type…"

    override fun getFamilyName(): String = "Import type"

    override fun startInWriteAction(): Boolean = false

    override fun isAvailable(project: Project, editor: Editor?, file: PsiFile?): Boolean {
        if (file !is JuxFile || editor == null) return false
        val ref = typeRefAtCaret(editor, file) ?: return false
        val name = simpleName(ref) ?: return false
        if (declaredInFile(file, name)) return false
        val fqns = candidates(project, file, name)
        cachedName = name
        cachedFqns = fqns
        return fqns.isNotEmpty()
    }

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?) {
        if (file !is JuxFile || editor == null) return
        val name = cachedName ?: return
        val fqns = cachedFqns
        when (fqns.size) {
            0 -> return
            1 -> doImport(project, editor, file, fqns[0], name)
            else -> JBPopupFactory.getInstance()
                .createPopupChooserBuilder(fqns)
                .setTitle("Import Type")
                .setItemChosenCallback { doImport(project, editor, file, it, name) }
                .createPopup()
                .showInBestPositionFor(editor)
        }
    }

    private fun doImport(project: Project, editor: Editor, file: JuxFile, fqn: String, name: String) {
        WriteCommandAction.runWriteCommandAction(project, "Import Type", null, {
            JuxAutoImport.addImport(project, editor.document, file, fqn, name)
        }, file)
    }

    // ---- analysis --------------------------------------------------------------

    /**
     * The innermost [E.TYPE_REFERENCE] covering the caret. Checks the leaf AT
     * the caret and the one just before it (a caret resting at the end of a
     * type name sits on the following whitespace), walking each up to its
     * enclosing type reference.
     */
    private fun typeRefAtCaret(editor: Editor, file: JuxFile): PsiElement? {
        val offset = editor.caretModel.offset
        return refFromLeaf(file.findElementAt(offset))
            ?: refFromLeaf(file.findElementAt(offset - 1))
    }

    private fun refFromLeaf(leaf: PsiElement?): PsiElement? {
        var e: PsiElement? = leaf
        while (e != null && e !is JuxFile) {
            if (e.elementType === E.TYPE_REFERENCE) return e
            e = e.parent
        }
        return null
    }

    /** Bare type name (generics stripped), or null when qualified (`a.b.T`). */
    private fun simpleName(ref: PsiElement): String? {
        val bare = ref.text.substringBefore('<').trim()
        return if (bare.isEmpty() || bare.contains('.')) null else bare
    }

    /** A type with [name] declared in this file needs no import. */
    private fun declaredInFile(file: JuxFile, name: String): Boolean =
        PsiTreeUtil.findChildrenOfType(file, JuxTypeDeclaration::class.java).any { it.name == name }

    /**
     * FQNs of project types named [name] that live in a different, named
     * package than [file] and aren't imported yet — the import candidates.
     */
    private fun candidates(project: Project, file: JuxFile, name: String): List<String> {
        val curPkg = JuxAutoImport.packageOfFile(file)
        val out = LinkedHashSet<String>()
        JuxTypeIndex.forEachType(project, GlobalSearchScope.allScope(project)) { type ->
            if (type.name != name) return@forEachType
            val pkg = JuxAutoImport.packageOf(type)
            if (pkg.isEmpty() || pkg == curPkg) return@forEachType
            val fqn = "$pkg.$name"
            if (!JuxAutoImport.isImported(file, fqn, name)) out.add(fqn)
        }
        return out.toList()
    }
}
