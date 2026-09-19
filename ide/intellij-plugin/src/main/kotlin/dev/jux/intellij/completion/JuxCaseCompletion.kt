package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.PrioritizedLookupElement
import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.icons.AllIcons
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.intentions.JuxSwitchCases
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Completion right after `case` in a `switch` over an enum or a sealed type:
 * the constants and subtypes no arm names yet, as Java offers the missing
 * enum constants.
 *
 * Only what is still missing is offered, in declaration order, so the popup
 * doubles as a checklist: when it comes up empty the switch is exhaustive.
 * A payload variant is written with one `_` per field and a sealed subtype
 * as a type pattern, the same labels "Create missing 'case' branches" writes.
 */
object JuxCaseCompletion {

    /**
     * Adds the missing labels when the caret starts a `case` pattern. Returns
     * true when it did, so the caller offers nothing else (only a pattern can
     * go there, and every useful pattern is in the list).
     */
    fun addTo(parameters: CompletionParameters, sink: (LookupElement) -> Unit): Boolean {
        val position = parameters.position
        val switch = switchAfterCase(position) ?: return false
        val labels = JuxSwitchCases.missing(switch)
        if (labels.isEmpty()) return false
        val count = labels.size
        for ((i, label) in labels.withIndex()) {
            val item = LookupElementBuilder.create(label.text)
                .withLookupString(label.name)
                .withPresentableText(label.text)
                .withTypeText("missing case", true)
                .withIcon(AllIcons.Nodes.Enum)
            // Declaration order survives the relevance sorter: earlier is higher.
            sink(PrioritizedLookupElement.withPriority(item, 1000.0 + (count - i)))
        }
        return true
    }

    /**
     * The switch whose arm the caret starts, when the caret directly follows
     * `case` (or a `,` / `|` that separates alternatives of that arm).
     */
    fun switchAfterCase(position: PsiElement): PsiElement? {
        val prev = PsiTreeUtil.prevVisibleLeaf(position) ?: return null
        val arm = PsiTreeUtil.findFirstParent(position) { it.elementType === E.SWITCH_CASE }
        val startsArm = prev.elementType === T.CASE_KW ||
            (arm != null && (prev.elementType === T.COMMA || prev.elementType === T.PIPE) &&
                PsiTreeUtil.isAncestor(arm, prev, true) && !afterArrow(arm, prev))
        if (!startsArm) return null
        return PsiTreeUtil.findFirstParent(position) {
            it.elementType === E.SWITCH_STATEMENT || it.elementType === E.SWITCH_EXPRESSION
        }
    }

    /** True when [leaf] sits in the arm's body, past its `->` or `:`. */
    private fun afterArrow(arm: PsiElement, leaf: PsiElement): Boolean {
        var c: PsiElement? = arm.firstChild
        while (c != null && c.textRange.startOffset < leaf.textRange.startOffset) {
            if (c.elementType === T.ARROW || c.elementType === T.COLON) return true
            c = c.nextSibling
        }
        return false
    }
}
