package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiFile
import dev.jux.intellij.psi.JuxFile

/**
 * Flags the misplaced-accessor mistake `T Name = { get; set; };` — the accessor
 * block written AFTER the `=`. The correct Jux form puts the block first:
 * `T Name { get; set; } [= init];`.
 *
 * This mirrors the compiler's `E0200` ("the accessor block must come before
 * `=`") so the IDE offers a one-click fix even though the malformed text parses
 * as a field with a block initializer. Detection is a small text scan rather
 * than PSI-shape matching, because the malformed input has no clean accessor
 * PSI to anchor on.
 */
class JuxMisplacedAccessorBlockInspection : LocalInspectionTool() {

    override fun checkFile(
        file: PsiFile,
        manager: InspectionManager,
        isOnTheFly: Boolean,
    ): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val text = file.text
        val problems = ArrayList<ProblemDescriptor>()
        for (m in ACCESSOR_AFTER_EQ.findAll(text)) {
            val braceOffset = m.range.first + m.value.indexOf('{')
            val anchor = file.findElementAt(braceOffset) ?: continue
            problems.add(
                manager.createProblemDescriptor(
                    anchor,
                    "Accessor block must come before `=` — write `Name { get; set; } = init;`",
                    RemoveEqBeforeAccessorFix(),
                    ProblemHighlightType.GENERIC_ERROR_OR_WARNING,
                    isOnTheFly,
                ),
            )
        }
        return problems.toTypedArray()
    }

    /** Delete the stray `=` between the property name and the accessor block. */
    private class RemoveEqBeforeAccessorFix : LocalQuickFix {
        override fun getFamilyName(): String = "Remove `=` before the accessor block"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val brace = descriptor.psiElement ?: return
            val file = brace.containingFile ?: return
            val docManager = PsiDocumentManager.getInstance(project)
            val doc = docManager.getDocument(file) ?: return
            val chars = doc.charsSequence
            val braceStart = brace.textRange.startOffset
            // Walk back over whitespace to the `=`, then replace `= … {`'s `=`
            // (and the gap up to the brace) with a single space.
            var i = braceStart - 1
            while (i >= 0 && chars[i].isWhitespace()) i--
            if (i >= 0 && chars[i] == '=') {
                doc.replaceString(i, braceStart, " ")
                docManager.commitDocument(doc)
            }
        }
    }

    private companion object {
        // `= {` followed by an optional accessor visibility and then `get`/`set`.
        val ACCESSOR_AFTER_EQ = Regex(
            """=\s*\{\s*(?:(?:public|private|protected)\s+)?(?:get|set)\b""",
        )
    }
}
