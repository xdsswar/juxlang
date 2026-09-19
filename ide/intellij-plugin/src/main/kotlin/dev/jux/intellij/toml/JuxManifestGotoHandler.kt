package dev.jux.intellij.toml

import com.intellij.codeInsight.navigation.actions.GotoDeclarationHandler
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiManager
import com.intellij.psi.impl.FakePsiElement

/**
 * Ctrl+B on an inherited `jux.toml` entry (`edition.workspace = true`, or a
 * dependency's `workspace = true`) jumps to where the value really is: the
 * key in the workspace root's `[workspace.package]` or
 * `[workspace.dependencies]` (JUX-BUILD-SYSTEM-ADDENDUM §B.7).
 *
 * The target is a small navigatable element at the key's offset, so it lands
 * on the right line whether the root is read as TOML or as plain text.
 */
class JuxManifestGotoHandler : GotoDeclarationHandler {

    override fun getGotoDeclarationTargets(sourceElement: PsiElement?, offset: Int, editor: Editor?): Array<PsiElement>? {
        val file = sourceElement?.containingFile ?: return null
        if (!JuxManifestFiles.isManifest(file)) return null
        val vf = file.virtualFile ?: return null
        val text = file.text
        val entry = JuxTomlModel.entries(text).firstOrNull {
            it.inheritsFromWorkspace && offset >= it.keyStart && offset <= it.valueEnd
        } ?: return null
        val root = JuxManifestFiles.workspaceRoot(vf) ?: return null
        val rootText = if (root == vf) text else JuxManifestFiles.textOf(root)
        val at = JuxTomlModel.workspaceDeclarationOffset(rootText, entry.table, entry.head) ?: return null
        val rootPsi = PsiManager.getInstance(file.project).findFile(root) ?: return null
        return arrayOf(ManifestKeyTarget(rootPsi, at, entry.head))
    }

    /** A navigatable point in a manifest: the offset of one key. */
    class ManifestKeyTarget(private val file: PsiFile, private val offset: Int, private val key: String) : FakePsiElement() {
        override fun getParent(): PsiElement = file
        override fun getContainingFile(): PsiFile = file
        override fun getName(): String = key
        override fun getTextOffset(): Int = offset
        override fun getTextRange(): TextRange = TextRange(offset, offset + key.length)
        override fun canNavigate(): Boolean = true
        override fun canNavigateToSource(): Boolean = true

        override fun navigate(requestFocus: Boolean) {
            val vf = file.virtualFile ?: return
            OpenFileDescriptor(file.project, vf, offset).navigate(requestFocus)
        }

        /** Where the target points, for tests and the popup. */
        fun offset(): Int = offset
    }
}
