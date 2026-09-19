package dev.jux.intellij.intentions

import com.intellij.openapi.editor.Editor
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.intentions.JuxCodeShapes as S
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Create missing 'case' branches, Java's "Create missing switch branches":
 * on a `switch` over an enum or a sealed type with no `default`, adds one arm
 * for every constant or permitted subtype that no arm names yet.
 *
 * - an enum constant gets `case Name`, a payload variant `case Name(_, _)`
 *   with one `_` per field;
 * - a sealed type's subtype gets a type pattern, `case Circle circle`;
 * - a switch statement's new arm is an empty block, a switch expression's
 *   arm throws, so the result compiles and every new arm is easy to find.
 *
 * The caret must be on the `switch` keyword or its subject. What is missing
 * comes from [JuxSwitchCases], shared with `case` completion and the
 * `.switch` postfix template.
 */
class JuxCreateMissingCasesIntention : JuxIntention("Create missing 'case' branches") {

    override fun target(element: PsiElement): PsiElement? {
        val switch = ancestor(element, E.SWITCH_STATEMENT, E.SWITCH_EXPRESSION) ?: return null
        val subject = JuxSwitchCases.subjectOf(switch) ?: return null
        val onHeader = element.parent === switch && element.elementType === T.SWITCH_KW ||
            PsiTreeUtil.isAncestor(subject, element, false)
        if (!onHeader) return null
        if (JuxSwitchCases.hasDefault(switch)) return null
        return switch.takeIf { JuxSwitchCases.missing(it).isNotEmpty() }
    }

    override fun textFor(target: PsiElement): String {
        val n = JuxSwitchCases.missing(target).size
        return if (n == 1) "Create missing 'case' branch" else "Create $n missing 'case' branches"
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val labels = JuxSwitchCases.missing(target)
        if (labels.isEmpty()) return
        val close = target.lastChild?.takeIf { it.elementType === T.RBRACE } ?: return
        val arms = JuxSwitchCases.arms(labels, expression = target.elementType === E.SWITCH_EXPRESSION)
        S.replace(target, TextRange(close.textRange.startOffset, close.textRange.startOffset), arms)
    }
}
