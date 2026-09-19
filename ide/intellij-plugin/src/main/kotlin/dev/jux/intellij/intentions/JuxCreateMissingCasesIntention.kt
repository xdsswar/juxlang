package dev.jux.intellij.intentions

import com.intellij.openapi.editor.Editor
import com.intellij.openapi.util.TextRange
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
import dev.jux.intellij.resolve.JuxTypeEngine

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
 * The caret must be on the `switch` keyword or its subject.
 */
class JuxCreateMissingCasesIntention : JuxIntention("Create missing 'case' branches") {

    override fun target(element: PsiElement): PsiElement? {
        val switch = ancestor(element, E.SWITCH_STATEMENT, E.SWITCH_EXPRESSION) ?: return null
        val subject = subjectOf(switch) ?: return null
        val onHeader = element.parent === switch && element.elementType === T.SWITCH_KW ||
            PsiTreeUtil.isAncestor(subject, element, false)
        if (!onHeader) return null
        if (hasDefault(switch)) return null
        return switch.takeIf { missingLabels(it).isNotEmpty() }
    }

    override fun textFor(target: PsiElement): String {
        val n = missingLabels(target).size
        return if (n == 1) "Create missing 'case' branch" else "Create $n missing 'case' branches"
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val labels = missingLabels(target)
        if (labels.isEmpty()) return
        val close = target.lastChild?.takeIf { it.elementType === T.RBRACE } ?: return
        val expression = target.elementType === E.SWITCH_EXPRESSION
        val arms = labels.joinToString("") { label ->
            val body = if (expression) "throw new UnsupportedOperationException(\"TODO: $label\");" else "{\n}"
            "case $label -> $body\n"
        }
        S.replace(target, TextRange(close.textRange.startOffset, close.textRange.startOffset), arms)
    }

    // ---- what is missing -----------------------------------------------------

    private fun subjectOf(switch: PsiElement): PsiElement? = S.compositeAfter(switch, T.LPAREN)

    private fun hasDefault(switch: PsiElement): Boolean =
        switch.children.any { it.elementType === E.SWITCH_CASE && it.node.findChildByType(T.DEFAULT_KW) != null }

    /** Every name an existing arm's patterns mention: `Red`, `Color.Red`, `Circle c`, `Circle(var r)`. */
    private fun coveredNames(switch: PsiElement): Set<String> {
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

    /** The case labels to add, in declaration order. Empty when the subject is neither an enum nor sealed. */
    private fun missingLabels(switch: PsiElement): List<String> {
        val subject = subjectOf(switch) ?: return emptyList()
        val type = JuxTypeEngine.classOf(JuxTypeEngine.typeOf(subject))?.decl ?: return emptyList()
        val covered = coveredNames(switch)
        return when {
            type.node.elementType === E.ENUM_DECLARATION -> enumLabels(type).filter { (name, _) -> name !in covered }
                .map { it.second }
            JuxHierarchy.hasModifier(type, "sealed") -> sealedLabels(type).filter { (name, _) -> name !in covered }
                .map { it.second }
            else -> emptyList()
        }
    }

    /** `(name, label)` for each constant: `Red` / `Circle(_)` for a payload variant. */
    private fun enumLabels(type: JuxTypeDeclaration): List<Pair<String, String>> =
        PsiTreeUtil.collectElements(type) { it.elementType === E.ENUM_CONSTANT && JuxHierarchy.enclosingType(it) === type }
            .mapNotNull { c ->
                val name = generateSequence(c.firstChild) { it.nextSibling }
                    .firstOrNull { it.elementType === T.IDENTIFIER }?.text ?: return@mapNotNull null
                val payload = payloadArity(c)
                name to if (payload == 0) name else "$name(${List(payload) { "_" }.joinToString(", ")})"
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

    /** `(name, label)` for each direct subtype of a sealed type: `Circle circle`. */
    private fun sealedLabels(type: JuxTypeDeclaration): List<Pair<String, String>> {
        val subtypes = JuxSubtypes.directSubtypes(type, JuxSubtypes.buildIndex(type.project))
        return subtypes.mapNotNull { sub ->
            val name = (sub as JuxNamedElement).name ?: return@mapNotNull null
            name to "$name ${name.replaceFirstChar { it.lowercase() }}"
        }.distinctBy { it.first }
    }
}
