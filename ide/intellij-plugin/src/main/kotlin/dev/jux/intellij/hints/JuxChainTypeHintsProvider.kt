package dev.jux.intellij.hints

import com.intellij.codeInsight.hints.declarative.HintFormat
import com.intellij.codeInsight.hints.declarative.InlayHintsCollector
import com.intellij.codeInsight.hints.declarative.InlayHintsProvider
import com.intellij.codeInsight.hints.declarative.InlayTreeSink
import com.intellij.codeInsight.hints.declarative.InlineInlayPosition
import com.intellij.codeInsight.hints.declarative.SharedBypassCollector
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * Method-chain type hints, Java's "Method chains" hints: in a call chain
 * written one call per line, the type each line produces, at the end of
 * that line.
 *
 * ```
 * var names = people        //
 *     .iter()               // Iterator<Person>
 *     .map(p -> p.name)     // Iterator<String>
 *     .collect();
 * ```
 *
 * As in Java, only a chain worth reading this way gets hints: three calls or
 * more, spread over lines, producing at least two different types (a builder
 * that returns itself every time says nothing new). An unknown type gets no
 * hint rather than a `?`.
 */
class JuxChainTypeHintsProvider : InlayHintsProvider {

    override fun createCollector(file: PsiFile, editor: Editor): InlayHintsCollector? {
        if (file !is JuxFile) return null
        return Collector
    }

    private object Collector : SharedBypassCollector {
        override fun collectFromElement(element: PsiElement, sink: InlayTreeSink) {
            for ((offset, text) in hintsFor(element)) {
                sink.addPresentation(InlineInlayPosition(offset, true), hintFormat = HintFormat.default) { text(text) }
            }
        }
    }

    companion object {
        private const val MIN_CALLS = 3

        /**
         * The (offset, text) hints for the chain [top] is the outermost call
         * of, or none when [top] is not such a call or the chain does not
         * qualify. Public for its tests.
         */
        fun hintsFor(top: PsiElement): List<Pair<Int, String>> {
            if (top.elementType !== E.CALL_EXPRESSION) return emptyList()
            if (isInsideChain(top)) return emptyList()
            val calls = chainCalls(top)
            if (calls.size < MIN_CALLS) return emptyList()
            val file = top.containingFile.text
            val hints = ArrayList<Pair<Int, String>>()
            val types = HashSet<String>()
            for (call in calls) {
                val end = call.textRange.endOffset
                if (!lineBreakBeforeNextDot(file, end)) continue
                val type = JuxTypeEngine.typeOf(call)
                if (hasUnknown(type)) continue
                val text = type.presentable()
                types.add(text)
                hints.add(end to text)
            }
            return if (hints.size >= 2 && types.size >= 2) hints else emptyList()
        }

        /** True when [call] is itself a link of a longer chain (`call.next()`). */
        private fun isInsideChain(call: PsiElement): Boolean {
            val parent = call.parent ?: return false
            return parent.elementType === E.FIELD_ACCESS_EXPRESSION &&
                JuxTypeEngine.firstExpressionChild(parent) === call
        }

        /** The calls of the chain ending in [top], innermost first. */
        private fun chainCalls(top: PsiElement): List<PsiElement> {
            val out = ArrayList<PsiElement>()
            var current: PsiElement? = top
            while (current != null && current.elementType === E.CALL_EXPRESSION) {
                out.add(current)
                val callee = JuxTypeEngine.firstExpressionChild(current) ?: break
                if (callee.elementType !== E.FIELD_ACCESS_EXPRESSION) break
                current = JuxTypeEngine.firstExpressionChild(callee)
            }
            return out.reversed()
        }

        /** Whether the text after [offset] reaches a line break before anything but blanks. */
        private fun lineBreakBeforeNextDot(text: String, offset: Int): Boolean {
            var i = offset
            while (i < text.length && (text[i] == ' ' || text[i] == '\t' || text[i] == '\r')) i++
            if (i >= text.length || text[i] != '\n') return false
            while (i < text.length && text[i].isWhitespace()) i++
            return i < text.length && text[i] == '.'
        }

        private fun hasUnknown(t: JuxType): Boolean = when (t) {
            is JuxType.Unknown -> true
            is JuxType.ClassType -> t.args.any { hasUnknown(it) }
            is JuxType.ArrayType -> hasUnknown(t.element)
            is JuxType.Nullable -> hasUnknown(t.inner)
            is JuxType.Static -> true
            else -> false
        }
    }
}
