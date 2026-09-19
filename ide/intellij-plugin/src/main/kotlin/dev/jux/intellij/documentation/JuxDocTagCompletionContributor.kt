package dev.jux.intellij.documentation

import com.intellij.codeInsight.completion.CompletionContributor
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionResultSet
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/**
 * Completion inside a `/** ... */` doc comment:
 *
 *  - after `@`: the tags `jux doc` renders (`param`, `return`, `throws`,
 *    `deprecated`, `since`, `see`, and their synonyms `returns` and
 *    `exception`);
 *  - after `@param `: the documented method's parameters (and `<T>` type
 *    parameters) that have no `@param` yet, most useful first;
 *  - after ```` ``` ```` on its own line: `jux`, `jux no_run`, `jux ignore`,
 *    the example-block forms `jux test --doc` understands.
 *
 * Skipped inside a fenced block, where the text is code, not tags.
 */
class JuxDocTagCompletionContributor : CompletionContributor() {

    override fun fillCompletionVariants(parameters: CompletionParameters, result: CompletionResultSet) {
        val comment = parameters.position.let { p -> p as? PsiComment ?: p.parent as? PsiComment } ?: return
        if (comment.elementType !== JuxTokenTypes.DOC_COMMENT) return
        val original = parameters.originalPosition?.let { it as? PsiComment ?: it.parent as? PsiComment } ?: comment
        val text = original.text
        val rel = (parameters.offset - original.textRange.startOffset).coerceIn(0, text.length)
        val lineStart = text.lastIndexOf('\n', rel - 1) + 1
        val linePrefix = text.substring(lineStart, rel)
        if (insideFence(text, lineStart)) return

        FENCE_PREFIX.matchEntire(linePrefix)?.let { m ->
            val set = result.withPrefixMatcher(m.groupValues[1])
            listOf("jux", "jux no_run", "jux ignore").forEach { set.addElement(LookupElementBuilder.create(it).bold()) }
            result.stopHere()
            return
        }
        PARAM_PREFIX.find(linePrefix)?.let { m ->
            val set = result.withPrefixMatcher(m.groupValues[1])
            val documented = JuxDocFences.tags(text).filter { it.name == "param" }.mapNotNull { it.value?.text }.toSet()
            parameterNames(original).filter { it !in documented }.forEachIndexed { i, name ->
                set.addElement(
                    com.intellij.codeInsight.completion.PrioritizedLookupElement.withPriority(
                        LookupElementBuilder.create(name).withTypeText("parameter", true),
                        -i.toDouble(),
                    ),
                )
            }
            result.stopHere()
            return
        }
        TAG_PREFIX.find(linePrefix)?.let { m ->
            val set = result.withPrefixMatcher(m.groupValues[1])
            JuxDocFences.TAGS.forEach { tag ->
                set.addElement(LookupElementBuilder.create(tag).withPresentableText("@$tag").withTailText(TAG_HINTS[tag], true))
            }
            result.stopHere()
        }
    }

    /** True when the line starting at [lineStart] is inside a fenced block. */
    private fun insideFence(text: String, lineStart: Int): Boolean {
        val before = text.substring(0, lineStart)
        return before.lines().count { line ->
            line.trimStart().removePrefix("/**").trimStart().removePrefix("*").trimStart().startsWith("```")
        } % 2 == 1
    }

    /** The documented declaration's parameter names, `<T>` type parameters first. */
    private fun parameterNames(comment: PsiElement): List<String> {
        var next = comment.nextSibling
        while (next is PsiWhiteSpace || next is PsiComment) next = next?.nextSibling
        val decl = next ?: return emptyList()
        val types = decl.node.findChildByType(E.TYPE_PARAMETER_LIST)?.psi?.children
            ?.filterIsInstance<JuxNamedElement>()?.mapNotNull { it.name?.let { n -> "<$n>" } }.orEmpty()
        val params = decl.node.findChildByType(E.PARAMETER_LIST)?.psi?.children
            ?.filter { it.elementType === E.PARAMETER }
            ?.mapNotNull { (it as? JuxNamedElement)?.name }.orEmpty()
        return params + types
    }

    private companion object {
        val TAG_PREFIX = Regex("""(?:^|\s)@([A-Za-z]*)$""")
        val PARAM_PREFIX = Regex("""@param\s+(<?[A-Za-z0-9_]*)$""")
        val FENCE_PREFIX = Regex("""^\s*(?:/\*\*|\*)?\s*```([A-Za-z_ ]*)$""")

        val TAG_HINTS = mapOf(
            "param" to " name text",
            "return" to " text",
            "returns" to " text",
            "throws" to " Type text",
            "exception" to " Type text",
            "deprecated" to " reason",
            "since" to " version",
            "see" to " reference",
        )
    }
}
