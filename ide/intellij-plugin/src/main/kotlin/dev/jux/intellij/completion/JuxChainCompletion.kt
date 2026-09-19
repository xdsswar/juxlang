package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.PrioritizedLookupElement
import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.icons.AllIcons
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxMember
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * Chain completion, Java's second smart completion: where a value of some
 * type is wanted and no variable in reach has it, the chains that get there
 * from one that does, `order.customer().address()` for an `Address` slot.
 *
 * A link is a field, a property, a record component, or a method that takes
 * no arguments and returns something (the getter shape); a chain is at most
 * two links long. Starting points are the locals, parameters and fields the
 * smart completion already walked. Chains rank below the values that fit
 * outright, shortest first.
 */
object JuxChainCompletion {

    private const val MAX_RESULTS = 40

    /** Adds the chains from [roots] (lookup items for variables in reach) that reach [expected]. */
    fun addTo(expected: List<JuxType>, roots: List<LookupElement>, sink: (LookupElement) -> Unit) {
        if (expected.isEmpty()) return
        var added = 0
        val seen = HashSet<String>()
        for (root in roots) {
            val decl = root.psiElement ?: continue
            if (decl.elementType !in VARIABLES) continue
            val rootType = JuxCompletionRanking.typeOf(root)
            if (rootType is JuxType.Unknown || JuxCompletionRanking.bestFit(rootType, expected) == 0) continue
            val rootText = root.lookupString
            for ((first, firstType) in links(rootType)) {
                val one = "$rootText.$first"
                if (JuxCompletionRanking.bestFit(firstType, expected) == 0) {
                    if (seen.add(one)) {
                        sink(item(one, firstType, depth = 1))
                        if (++added >= MAX_RESULTS) return
                    }
                    continue
                }
                for ((second, secondType) in links(firstType)) {
                    if (JuxCompletionRanking.bestFit(secondType, expected) != 0) continue
                    val two = "$one.$second"
                    if (seen.add(two)) {
                        sink(item(two, secondType, depth = 2))
                        if (++added >= MAX_RESULTS) return
                    }
                }
            }
        }
    }

    /** The one-step links out of a value of [type]: `name` or `name()`, with the type each gives. */
    private fun links(type: JuxType): List<Pair<String, JuxType>> {
        if (JuxTypeEngine.classOf(type) == null) return emptyList()
        val out = ArrayList<Pair<String, JuxType>>()
        for (member in JuxTypeEngine.membersOf(type)) {
            val m = member.element
            if (JuxTypeEngine.isStaticMember(m)) continue
            if (JuxHierarchy.hasModifier(m, "private")) continue
            val name = (m as? JuxNamedElement)?.name ?: continue
            when (m.elementType) {
                E.FIELD_DECLARATION, E.PROPERTY_DECLARATION, E.RECORD_COMPONENT ->
                    out.add(name to JuxTypeEngine.memberType(member))
                E.METHOD_DECLARATION -> {
                    if (JuxHierarchy.arity(m) != 0) continue
                    val returned = returnOf(member)
                    if (returned is JuxType.Unknown || (returned is JuxType.Primitive && returned.name == "void")) continue
                    out.add("$name()" to returned)
                }
            }
        }
        return out
    }

    private fun returnOf(member: JuxMember): JuxType = JuxTypeEngine.returnType(member)

    private fun item(text: String, type: JuxType, depth: Int): LookupElement {
        val builder = LookupElementBuilder.create(text)
            .withTypeText(type.presentable(), true)
            .withIcon(AllIcons.Nodes.Method)
            .withTailText("  chain", true)
        // Below everything that fits outright; a shorter chain above a longer one.
        return PrioritizedLookupElement.withPriority(builder, -10.0 * depth)
    }

    private val VARIABLES = setOf(
        E.LOCAL_VARIABLE, E.PARAMETER, E.FIELD_DECLARATION, E.PROPERTY_DECLARATION, E.RECORD_COMPONENT,
    )

    /** For tests: whether [element] could start a chain. */
    internal fun isVariable(element: PsiElement?): Boolean = element?.elementType in VARIABLES
}
