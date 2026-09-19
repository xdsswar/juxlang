package dev.jux.intellij.intentions

import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.intentions.JuxCodeShapes as S
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxSubtypes
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * What a `switch` over an enum or a sealed type could name in its arms, and
 * which of those it does not name yet.
 *
 * One source for three features that must agree: the "Create missing 'case'
 * branches" intention, completion right after `case`, and the `.switch`
 * postfix template, which writes every arm up front.
 *
 * A label is what goes after `case`:
 * - an enum constant is `Red`, a payload variant `Circle(_)` with one `_` per
 *   field, so the arm compiles before its binders are named;
 * - a sealed type's direct subtype is a type pattern, `Circle circle`.
 */
object JuxSwitchCases {

    /** One possible arm: the name it covers and the text written after `case`. */
    data class Label(val name: String, val text: String)

    /** The `switch`'s subject expression, the one in its parentheses. */
    fun subjectOf(switch: PsiElement): PsiElement? = S.compositeAfter(switch, T.LPAREN)

    /** True when an arm is `default`. */
    fun hasDefault(switch: PsiElement): Boolean =
        switch.children.any { it.elementType === E.SWITCH_CASE && it.node.findChildByType(T.DEFAULT_KW) != null }

    /** Every name an existing arm's patterns mention: `Red`, `Color.Red`, `Circle c`, `Circle(var r)`. */
    fun coveredNames(switch: PsiElement): Set<String> {
        val out = HashSet<String>()
        for (arm in switch.children) {
            if (arm.elementType !== E.SWITCH_CASE) continue
            // Only the patterns: the arm's body may mention a constant too.
            for (pattern in arm.children) {
                if (pattern.elementType !== E.PATTERN) continue
                PsiTreeUtil.collectElements(pattern) { it.elementType === T.IDENTIFIER }.forEach { out.add(it.text) }
            }
        }
        return out
    }

    /** The labels the switch does not cover yet, in declaration order. */
    fun missing(switch: PsiElement): List<Label> {
        val subject = subjectOf(switch) ?: return emptyList()
        val covered = coveredNames(switch)
        return labelsFor(JuxTypeEngine.typeOf(subject)).filter { it.name !in covered }
    }

    /** Every label of [type]: its enum constants or its sealed subtypes. Empty for anything else. */
    fun labelsFor(type: JuxType): List<Label> {
        val decl = JuxTypeEngine.classOf(type)?.decl ?: return emptyList()
        return labelsFor(decl)
    }

    /** Every label of [type]: its enum constants or its sealed subtypes. Empty for anything else. */
    fun labelsFor(type: JuxTypeDeclaration): List<Label> = when {
        type.node.elementType === E.ENUM_DECLARATION -> enumLabels(type)
        JuxHierarchy.hasModifier(type, "sealed") -> sealedLabels(type)
        else -> emptyList()
    }

    private fun enumLabels(type: JuxTypeDeclaration): List<Label> =
        PsiTreeUtil.collectElements(type) { it.elementType === E.ENUM_CONSTANT && JuxHierarchy.enclosingType(it) === type }
            .mapNotNull { c ->
                val name = generateSequence(c.firstChild) { it.nextSibling }
                    .firstOrNull { it.elementType === T.IDENTIFIER }?.text ?: return@mapNotNull null
                val payload = payloadArity(c)
                Label(name, if (payload == 0) name else "$name(${List(payload) { "_" }.joinToString(", ")})")
            }

    /** The number of fields a payload variant `Circle(double r)` declares, 0 for a plain constant. */
    private fun payloadArity(constant: PsiElement): Int {
        val text = constant.text
        val open = text.indexOf('(')
        if (open < 0) return 0
        val inner = text.substring(open + 1, text.lastIndexOf(')').takeIf { it > open } ?: text.length).trim()
        if (inner.isEmpty()) return 0
        var depth = 0
        var commas = 0
        for (ch in inner) {
            when (ch) {
                '(', '<', '[' -> depth++
                ')', '>', ']' -> depth--
                ',' -> if (depth == 0) commas++
            }
        }
        return commas + 1
    }

    private fun sealedLabels(type: JuxTypeDeclaration): List<Label> {
        val subtypes = JuxSubtypes.directSubtypes(type, JuxSubtypes.buildIndex(type.project))
        return subtypes.mapNotNull { sub ->
            val name = (sub as JuxNamedElement).name ?: return@mapNotNull null
            Label(name, "$name ${name.replaceFirstChar { it.lowercase() }}")
        }.distinctBy { it.name }
    }

    /**
     * The arms for [labels], one per line: an empty block in a switch
     * statement, a `throw` in a switch expression (so the result compiles and
     * every new arm is easy to find).
     */
    fun arms(labels: List<Label>, expression: Boolean): String = labels.joinToString("") { label ->
        val body = if (expression) "throw new UnsupportedOperationException(\"TODO: ${label.text}\");" else "{\n}"
        "case ${label.text} -> $body\n"
    }
}
