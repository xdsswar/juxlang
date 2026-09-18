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
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * The type a `var` local infers, shown after its name: `var total: int = …`,
 * as Java shows `var`'s implicit type.
 *
 * The type comes from the plugin's type engine, the same one member
 * completion uses, so the hint and the `.` popup never disagree. A hint that
 * would only repeat what the line already says is left out: the initializer
 * is `new T(…)`, a literal, or a cast, where the type is written right there.
 * An unknown type gets no hint rather than a `?`.
 */
class JuxVarTypeHintsProvider : InlayHintsProvider {

    override fun createCollector(file: PsiFile, editor: Editor): InlayHintsCollector? {
        if (file !is JuxFile) return null
        return Collector
    }

    private object Collector : SharedBypassCollector {
        override fun collectFromElement(element: PsiElement, sink: InlayTreeSink) {
            if (element.elementType !== E.LOCAL_VARIABLE) return
            val text = hintFor(element) ?: return
            val name = element.node.findChildByType(T.IDENTIFIER)?.psi ?: return
            sink.addPresentation(
                InlineInlayPosition(name.textRange.endOffset, true),
                hintFormat = HintFormat.default,
            ) {
                text(": $text")
            }
        }
    }

    companion object {
        /**
         * The hint text for a local, or null when it gets none: it has a
         * written type, its type is obvious from the initializer, or the
         * engine does not know it. Public for its tests.
         */
        fun hintFor(local: PsiElement): String? {
            if (local.node.findChildByType(T.VAR_KW) == null) return null
            if (local.node.findChildByType(E.TYPE_REFERENCE) != null) return null
            val initializer = initializerOf(local)
            if (initializer != null && initializer.elementType in OBVIOUS) return null
            val type = JuxTypeEngine.declaredType(local)
            if (type is JuxType.Static || hasUnknown(type)) return null
            return type.presentable()
        }

        /** A half-known type (`Vec<?>`) would print a `?` the language does not have. */
        private fun hasUnknown(t: JuxType): Boolean = when (t) {
            is JuxType.Unknown -> true
            is JuxType.ClassType -> t.args.any { hasUnknown(it) }
            is JuxType.ArrayType -> hasUnknown(t.element)
            is JuxType.Nullable -> hasUnknown(t.inner)
            else -> false
        }

        /** Initializers whose type is written in the initializer itself. */
        private val OBVIOUS = setOf(E.NEW_EXPRESSION, E.LITERAL_EXPRESSION, E.CAST_EXPRESSION)

        private fun initializerOf(local: PsiElement): PsiElement? {
            var sawEq = false
            var c: PsiElement? = local.firstChild
            while (c != null) {
                if (c.elementType === T.EQ) sawEq = true
                else if (sawEq && JuxTypeEngine.isExpression(c)) return c
                c = c.nextSibling
            }
            return null
        }
    }
}
