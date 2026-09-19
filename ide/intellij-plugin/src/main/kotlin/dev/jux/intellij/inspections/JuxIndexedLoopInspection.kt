package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.intentions.JuxCodeShapes as S
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * "Indexed loop can be a for-each", Java's "'for' loop can be replaced with
 * enhanced 'for'": a loop that counts an index over a whole sequence and only
 * ever reads `xs[i]` does not need the index.
 *
 * Both counting shapes are recognised:
 *
 * - `for (int i = 0; i < xs.len(); i++)` (also `++i`, `i += 1`, and
 *   `xs.length` for an array);
 * - `for (var i : 0..xs.len())`.
 *
 * The loop is reported only when every use of `i` in the body is the index of
 * a read `xs[i]` (never written, never used on its own), and the body never
 * assigns `xs` itself. The fix rewrites the header to `for (var x : xs)` and
 * each `xs[i]` to `x`.
 */
class JuxIndexedLoopInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                val t = element.elementType
                if (t !== E.FOR_STATEMENT && t !== E.FOR_EACH_STATEMENT) return
                val loop = analyze(element) ?: return
                val kw = S.token(element, T.FOR_KW) ?: return
                holder.registerProblem(kw, "Indexed loop can be a for-each over '${loop.sequence}'",
                    ProblemHighlightType.GENERIC_ERROR_OR_WARNING, ForEachFix())
            }
        }

    /** A loop that qualifies: the sequence's name, the index's name, and each `xs[i]` read. */
    private class Loop(val sequence: String, val index: String, val reads: List<PsiElement>, val body: PsiElement)

    companion object {
        /** The loop's shape when it can become a for-each, or null. */
        private fun analyze(loop: PsiElement): Loop? {
            val (index, sequence) = header(loop) ?: return null
            val body = S.loopBody(loop) ?: return null
            val reads = ArrayList<PsiElement>()
            var ok = true
            PsiTreeUtil.processElements(body) { e ->
                if (e.elementType === E.REFERENCE_EXPRESSION) {
                    val name = JuxTypeEngine.memberName(e)
                    if (name == index) {
                        val access = e.parent
                        if (access?.elementType === E.INDEX_EXPRESSION && isRead(access) &&
                            S.compositeChildren(access).let { it.size == 2 && it[1] === e && it[0].text == sequence }
                        ) {
                            reads.add(access)
                        } else {
                            ok = false
                        }
                    } else if (name == sequence && isWritten(e)) {
                        ok = false
                    }
                }
                // A nested lambda may run later, after the loop moved on: leave it be.
                if (e.elementType === E.LAMBDA_EXPRESSION) ok = false
                ok
            }
            if (!ok || reads.isEmpty()) return null
            return Loop(sequence, index, reads, body)
        }

        /**
         * `(i, xs)` of a counting header over the whole of `xs`, or null. `xs`
         * must be a plain name: a call or a field chain could differ per turn.
         */
        private fun header(loop: PsiElement): Pair<String, String>? {
            if (loop.elementType === E.FOR_EACH_STATEMENT) {
                val local = loop.node.findChildByType(E.LOCAL_VARIABLE)?.psi as? JuxNamedElement ?: return null
                val range = S.compositeAfter(loop, T.COLON)?.takeIf { it.elementType === E.RANGE_EXPRESSION } ?: return null
                if (S.token(range, T.DOT_DOT) == null || range.text.contains("step")) return null
                val ends = S.compositeChildren(range)
                if (ends.size != 2 || ends[0].text != "0") return null
                val seq = lengthOf(ends[1]) ?: return null
                return (local.name ?: return null) to seq
            }
            val init = loop.node.findChildByType(E.LOCAL_VARIABLE)?.psi as? JuxNamedElement ?: return null
            val index = init.name ?: return null
            val start = initValue(init) ?: return null
            if (start.text != "0") return null
            val parts = S.compositeChildren(loop).filter { it !== init }
            val cond = parts.getOrNull(0)?.takeIf { it.elementType === E.BINARY_EXPRESSION } ?: return null
            val update = parts.getOrNull(1) ?: return null
            if (S.operator(cond)?.elementType !== T.LT || S.left(cond)?.text != index) return null
            val seq = S.right(cond)?.let { lengthOf(it) } ?: return null
            val step = update.text.replace(" ", "")
            if (step != "$index++" && step != "++$index" && step != "$index+=1") return null
            // The update and condition must be the only other header parts.
            if (parts.size < 3) return null
            return index to seq
        }

        /** `xs` of `xs.len()` / `xs.length`, when `xs` is a plain name. */
        private fun lengthOf(e: PsiElement): String? {
            val (receiver, member) = when (e.elementType) {
                E.CALL_EXPRESSION -> {
                    if (JuxTypeEngine.argumentCount(e) != 0) return null
                    val callee = e.firstChild?.takeIf { it.elementType === E.FIELD_ACCESS_EXPRESSION } ?: return null
                    JuxTypeEngine.firstExpressionChild(callee) to JuxTypeEngine.memberName(callee)
                }
                E.FIELD_ACCESS_EXPRESSION -> JuxTypeEngine.firstExpressionChild(e) to JuxTypeEngine.memberName(e)
                else -> return null
            }
            if (receiver?.elementType !== E.REFERENCE_EXPRESSION) return null
            val ok = (e.elementType === E.CALL_EXPRESSION && member == "len") ||
                (e.elementType === E.FIELD_ACCESS_EXPRESSION && member == "length")
            return receiver.text.takeIf { ok }
        }

        private fun initValue(local: PsiElement): PsiElement? = S.compositeAfter(local, T.EQ)

        /** Whether an index access is only read: not assigned to, incremented, or passed as `out`/`ref`. */
        private fun isRead(access: PsiElement): Boolean {
            val parent = access.parent ?: return true
            if (parent.elementType === E.ASSIGNMENT_EXPRESSION && S.compositeChildren(parent).firstOrNull() === access) return false
            if (parent.elementType === E.POSTFIX_EXPRESSION || parent.elementType === E.UNARY_EXPRESSION) {
                val op = parent.node.getChildren(null).firstOrNull { it.elementType === T.PLUS_PLUS || it.elementType === T.MINUS_MINUS }
                if (op != null) return false
            }
            val before = PsiTreeUtil.prevVisibleLeaf(access)?.text
            return before != "out" && before != "ref"
        }

        /** Whether the name [ref] is assigned (`xs = ...`). */
        private fun isWritten(ref: PsiElement): Boolean {
            val parent = ref.parent ?: return false
            return parent.elementType === E.ASSIGNMENT_EXPRESSION && S.compositeChildren(parent).firstOrNull() === ref
        }

        /** A name for the element: `item` for `items`, `x` for `xs`, else `element`; never one already used. */
        private fun elementName(loop: Loop): String {
            val base = when {
                loop.sequence.length > 1 && loop.sequence.endsWith("ies") -> loop.sequence.dropLast(3) + "y"
                loop.sequence.length > 1 && loop.sequence.endsWith("s") -> loop.sequence.dropLast(1)
                else -> "element"
            }
            val used = PsiTreeUtil.collectElements(loop.body) { it.elementType === T.IDENTIFIER }.map { it.text }.toSet()
            var name = base
            var n = 2
            while (name in used || name == loop.sequence || name == loop.index) name = base + n++
            return name
        }
    }

    /** Rewrites the header to `for (var x : xs)` and each `xs[i]` to `x`. */
    private class ForEachFix : LocalQuickFix {
        override fun getFamilyName(): String = "Replace with for-each"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val loop = descriptor.psiElement?.parent ?: return
            val shape = analyze(loop) ?: return
            val name = elementName(shape)
            val body = shape.body
            // Rewrite the body text first (reads replaced back to front), then the whole loop.
            val bodyStart = body.textRange.startOffset
            val sb = StringBuilder(body.text)
            for (read in shape.reads.sortedByDescending { it.textRange.startOffset }) {
                val r = read.textRange.shiftLeft(bodyStart)
                sb.replace(r.startOffset, r.endOffset, name)
            }
            S.replace(loop, TextRange(loop.textRange.startOffset, loop.textRange.endOffset),
                "for (var $name : ${shape.sequence}) $sb")
        }
    }
}
