package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.editor.Document
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * W0820 (Layout-ABI §L.5.5): an `unsafe { }` block with no `// SAFETY:`
 * comment saying why its obligations hold.
 *
 * The same placements the compiler's lint accepts (`juxc-driver`'s
 * `safety_lint`), so the editor and `jux check` agree line for line:
 *
 * - a run of comment lines directly above the `unsafe` line (a blank line
 *   cuts the run off), or
 * - comment lines at the very top of the block, starting on the `{` line.
 *
 * Only blocks are linted: `unsafe native { }` and `unsafe` functions are
 * declarations, not blocks. The quick-fix writes `// SAFETY: ` above the
 * block, indented like it, with the caret after the colon.
 */
class JuxSafetyCommentInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.UNSAFE_STATEMENT) return
                val kw = element.firstChild?.takeIf { it.elementType === T.UNSAFE_KW } ?: return
                val block = element.node.findChildByType(E.CODE_BLOCK)?.psi ?: return
                val doc = PsiDocumentManager.getInstance(element.project).getDocument(element.containingFile) ?: return
                val kwLine = doc.getLineNumber(kw.textRange.startOffset)
                val braceLine = doc.getLineNumber(block.textRange.startOffset)
                if (justifiedAbove(doc, kwLine) || justifiedInside(doc, braceLine)) return
                holder.registerProblem(
                    kw,
                    "This `unsafe` block has no `// SAFETY:` comment: say why its obligations hold " +
                        "(the pointer is valid, the memory is not aliased, ...) (W0820)",
                    ProblemHighlightType.WARNING,
                    AddSafetyCommentFix(),
                )
            }
        }

    private fun lineText(doc: Document, line: Int): String =
        doc.getText(com.intellij.openapi.util.TextRange(doc.getLineStartOffset(line), doc.getLineEndOffset(line)))

    /** The comment run touching [kwLine] from above mentions `SAFETY:`. */
    private fun justifiedAbove(doc: Document, kwLine: Int): Boolean {
        var i = kwLine
        while (i > 0) {
            i--
            val l = lineText(doc, i).trim()
            if (!isCommentLine(l)) return false
            if (l.contains(MARKER)) return true
        }
        return false
    }

    /** The rest of the `{` line, then the comment lines right after it, mention `SAFETY:`. */
    private fun justifiedInside(doc: Document, braceLine: Int): Boolean {
        val rest = lineText(doc, braceLine).substringAfter('{', "").trim()
        if (rest.contains(MARKER)) return true
        var i = braceLine + 1
        while (i < doc.lineCount) {
            val l = lineText(doc, i).trim()
            if (!isCommentLine(l)) return false
            if (l.contains(MARKER)) return true
            i++
        }
        return false
    }

    private fun isCommentLine(l: String): Boolean = l.startsWith("//") || l.startsWith("/*") || l.startsWith("*")

    /** Writes `// SAFETY: ` on its own line above the block, caret after it. */
    private class AddSafetyCommentFix : LocalQuickFix {
        override fun getFamilyName(): String = "Add '// SAFETY:' comment"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val kw = descriptor.psiElement ?: return
            val file = kw.containingFile ?: return
            val doc = PsiDocumentManager.getInstance(project).getDocument(file) ?: return
            val line = doc.getLineNumber(kw.textRange.startOffset)
            val lineStart = doc.getLineStartOffset(line)
            val indent = doc.getText(com.intellij.openapi.util.TextRange(lineStart, kw.textRange.startOffset))
                .takeWhile { it == ' ' || it == '\t' }
            val comment = "$indent// SAFETY: \n"
            doc.insertString(lineStart, comment)
            PsiDocumentManager.getInstance(project).commitDocument(doc)
            val caret = lineStart + comment.length - 1
            FileEditorManager.getInstance(project).selectedTextEditor
                ?.takeIf { it.document == doc }
                ?.caretModel?.moveToOffset(caret)
        }
    }

    private companion object {
        const val MARKER = "SAFETY:"
    }
}
