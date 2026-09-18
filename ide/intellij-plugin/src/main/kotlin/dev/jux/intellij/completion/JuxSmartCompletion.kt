package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionUtil
import com.intellij.codeInsight.completion.InsertionContext
import com.intellij.codeInsight.completion.util.ParenthesesInsertHandler
import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.icons.AllIcons
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxMember
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * The part of smart completion (Ctrl+Shift+Space) that is not already a name
 * in scope: the values a type itself provides for a slot of that type.
 *
 *  - `true` / `false` where a `bool` is wanted;
 *  - `null` where a nullable `T?` is wanted;
 *  - `Color.RED` and the other variants where an enum is wanted;
 *  - `new Point()` where a class, record or struct is wanted (not an
 *    interface or an abstract class, which cannot be instantiated);
 *  - the wanted type's own static fields and factory methods that produce
 *    it (`Point.ORIGIN`, `Config.defaults()`).
 *
 * Locals, parameters and members whose type fits come from the contributor's
 * normal walk, filtered by [JuxCompletionRanking.fit].
 */
object JuxSmartCompletion {

    /** Offer what the expected types themselves provide. */
    fun addTypeFitting(parameters: CompletionParameters, expected: List<JuxType>, sink: (LookupElement) -> Unit) {
        val seen = HashSet<String>()
        fun add(element: LookupElement) {
            if (seen.add(element.lookupString)) sink(element)
        }
        for (want in expected) {
            if (want is JuxType.Nullable) add(literal("null", want))
            when (val bare = JuxTypeEngine.stripNullable(want)) {
                is JuxType.Primitive ->
                    if (bare.name == "bool") {
                        add(literal("true", bare))
                        add(literal("false", bare))
                    }
                is JuxType.ClassType -> {
                    val decl = bare.decl
                    if (decl.elementType === E.ENUM_DECLARATION) {
                        enumVariants(decl, bare).forEach(::add)
                    } else if (instantiable(decl)) {
                        add(newInstance(decl, bare))
                    }
                    staticProducers(decl, bare, parameters).forEach(::add)
                }
                else -> {}
            }
        }
    }

    private fun literal(word: String, type: JuxType): LookupElement {
        val builder = LookupElementBuilder.create(word).bold()
        builder.putUserData(JuxCompletionRanking.TYPE, if (word == "null") JuxType.Nullable(JuxType.Unknown) else type)
        builder.putUserData(JuxCompletionRanking.KIND, JuxCompletionRanking.Kind.KEYWORD)
        return builder
    }

    /** `Color.RED`, found by typing either `Color` or `RED`. */
    private fun enumVariants(decl: JuxTypeDeclaration, type: JuxType.ClassType): List<LookupElement> {
        val owner = decl.name ?: return emptyList()
        return JuxHierarchy.allMembersDeclaredIn(decl).filter { it.elementType === E.ENUM_CONSTANT }.mapNotNull { c ->
            val name = (c as? JuxNamedElement)?.name ?: return@mapNotNull null
            val builder = LookupElementBuilder.create(CompletionUtil.getOriginalOrSelf(c), "$owner.$name")
                .withLookupStrings(listOf("$owner.$name", name))
                .withIcon(AllIcons.Nodes.Enum)
                .withTypeText(owner, true)
            builder.putUserData(JuxCompletionRanking.TYPE, type)
            builder.putUserData(JuxCompletionRanking.KIND, JuxCompletionRanking.Kind.MEMBER)
            builder
        }
    }

    /** Whether `new T()` can be written for [decl]. */
    private fun instantiable(decl: JuxTypeDeclaration): Boolean = when (decl.elementType) {
        E.CLASS_DECLARATION -> !JuxHierarchy.hasModifier(decl, "abstract")
        E.RECORD_DECLARATION, E.STRUCT_DECLARATION -> true
        else -> false
    }

    /**
     * `new Point()`, written with the wanted type's arguments (`new
     * Box<Truck>()`), the caret left inside the parentheses when a
     * constructor takes arguments.
     */
    private fun newInstance(decl: JuxTypeDeclaration, type: JuxType.ClassType): LookupElement {
        val typeText = type.presentable()
        val takesArgs = constructorTakesArguments(decl)
        val builder = LookupElementBuilder.create(CompletionUtil.getOriginalOrSelf(decl), "new $typeText")
            .withLookupStrings(listOf("new $typeText", decl.name ?: typeText))
            .withIcon(AllIcons.Nodes.Class)
            .withTailText("()", true)
            .withTypeText(typeText, true)
            .withInsertHandler { context: InsertionContext, _ ->
                val doc = context.document
                val end = context.tailOffset
                doc.insertString(end, "()")
                context.editor.caretModel.moveToOffset(if (takesArgs) end + 1 else end + 2)
            }
        builder.putUserData(JuxCompletionRanking.TYPE, type)
        builder.putUserData(JuxCompletionRanking.KIND, JuxCompletionRanking.Kind.TYPE_FILE)
        return builder
    }

    /** True when every way to construct [decl] takes an argument (a record's components count). */
    private fun constructorTakesArguments(decl: JuxTypeDeclaration): Boolean {
        val components = decl.node.findChildByType(E.RECORD_COMPONENT_LIST)?.psi?.children
            ?.count { it.elementType === E.RECORD_COMPONENT } ?: 0
        if (components > 0) return true
        val ctors = JuxHierarchy.allMembersDeclaredIn(decl).filter { it.elementType === E.CONSTRUCTOR_DECLARATION }
        return ctors.isNotEmpty() && ctors.none { JuxHierarchy.arity(it) == 0 }
    }

    /**
     * The wanted type's own statics that give a value of it: constants and
     * fields (`Point.ORIGIN`) and factory methods (`Config.defaults()`).
     */
    private fun staticProducers(
        decl: JuxTypeDeclaration,
        type: JuxType.ClassType,
        parameters: CompletionParameters,
    ): List<LookupElement> {
        val owner = decl.name ?: return emptyList()
        val from = com.intellij.psi.util.PsiTreeUtil.getParentOfType(parameters.position, JuxTypeDeclaration::class.java)
        val out = ArrayList<LookupElement>()
        for (m in JuxHierarchy.allMembersDeclaredIn(decl)) {
            if (m.elementType === E.ENUM_CONSTANT || !JuxTypeEngine.isStaticMember(m)) continue
            if (!JuxHierarchy.memberVisibleFrom(m, from)) continue
            val name = (m as? JuxNamedElement)?.name ?: continue
            val member = JuxMember(m, JuxTypeEngine.selfType(decl))
            val isMethod = m.elementType === E.METHOD_DECLARATION
            val produced = if (isMethod) JuxTypeEngine.returnType(member) else JuxTypeEngine.memberType(member)
            if (JuxCompletionRanking.fit(produced, type) != 0) continue
            var builder = LookupElementBuilder.create(CompletionUtil.getOriginalOrSelf(m), "$owner.$name")
                .withLookupStrings(listOf("$owner.$name", name))
                .withIcon(if (isMethod) AllIcons.Nodes.Method else AllIcons.Nodes.Field)
                .withTypeText(produced.presentable(), true)
            if (isMethod) {
                builder = builder.withTailText(
                    m.node.findChildByType(E.PARAMETER_LIST)?.text?.replace(Regex("\\s+"), " ") ?: "()",
                    true,
                ).withInsertHandler(ParenthesesInsertHandler.getInstance(JuxHierarchy.arity(m) > 0))
            }
            builder.putUserData(JuxCompletionRanking.TYPE, produced)
            builder.putUserData(JuxCompletionRanking.KIND, JuxCompletionRanking.Kind.MEMBER)
            out.add(builder)
        }
        return out
    }
}
