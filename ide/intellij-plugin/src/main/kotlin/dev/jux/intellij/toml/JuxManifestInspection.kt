package dev.jux.intellij.toml

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiFile

/**
 * Workspace checks for `jux.toml` (JUX-BUILD-SYSTEM-ADDENDUM §B.7), as you
 * type: unknown `[workspace]` keys, members that resolve to no package,
 * `default-members` that are not members, and `key.workspace = true` entries
 * the workspace root does not declare. The rules live in the pure
 * [JuxTomlChecks]; this only maps them onto the file.
 *
 * Registered without a language: `jux.toml` is TOML when the TOML plugin is
 * installed and plain text otherwise, and the check reads the text either
 * way. Every other file is skipped by name before anything is read.
 */
class JuxManifestInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (!JuxManifestFiles.isManifest(file)) return null
        val vf = file.virtualFile ?: file.originalFile.virtualFile ?: return null
        val dir = vf.parent ?: return null
        val text = file.text
        val root = JuxManifestFiles.workspaceRoot(vf)
        val rootText = when {
            root == null -> null
            root == vf -> text
            else -> JuxManifestFiles.textOf(root)
        }
        val problems = JuxTomlChecks.check(text, JuxManifestFiles.VirtualDirView(dir), rootText)
        if (problems.isEmpty()) return null
        return problems.mapNotNull { p ->
            val range = TextRange(p.start.coerceIn(0, text.length), p.end.coerceIn(0, text.length))
            if (range.isEmpty) return@mapNotNull null
            manager.createProblemDescriptor(
                file,
                range,
                p.message,
                when (p.level) {
                    JuxTomlChecks.Level.ERROR -> ProblemHighlightType.GENERIC_ERROR
                    JuxTomlChecks.Level.WARNING -> ProblemHighlightType.GENERIC_ERROR_OR_WARNING
                    JuxTomlChecks.Level.WEAK_WARNING -> ProblemHighlightType.WEAK_WARNING
                },
                isOnTheFly,
            )
        }.toTypedArray()
    }
}
