package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.CompletionLocation
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionSorter
import com.intellij.codeInsight.completion.CompletionStatistician
import com.intellij.codeInsight.completion.CompletionUtil
import com.intellij.codeInsight.completion.PrefixMatcher
import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.codeInsight.lookup.LookupElementDecorator
import com.intellij.codeInsight.lookup.LookupElementWeigher
import com.intellij.openapi.roots.ProjectFileIndex
import com.intellij.openapi.util.Key
import com.intellij.psi.PsiElement
import com.intellij.psi.statistics.StatisticsInfo
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxMember
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * How Jux completion orders its popup, modelled on Java's
 * (`JavaCompletionSorting`): the most relevant item on top, whatever order
 * the items were produced in.
 *
 * Precedence, highest first:
 *
 *  1. **Prefix-match quality**: an exact or start-of-name match beats a
 *     camel-hump or middle match (the platform's `prefix` classifier).
 *  2. **Expected type**: an item whose type fits what the caret position
 *     wants (`int n = |`, an argument, a `return`, a condition, an `==`
 *     operand) beats one that does not.
 *  3. **Locality**: locals, nearest declaration first, then parameters, the
 *     enclosing type's own members, inherited members, top-level functions,
 *     keywords, then types. The type tiers follow how far the name is from
 *     the caret in the language's own terms: this file's declarations and its
 *     type parameters, this file's package (no `import` needed, §4.4), the
 *     built-in primitives, what this file already imports, the auto-prelude,
 *     and last everything that accepting would have to write an `import` for.
 *  4. **Usage statistics**: what the user picked before in the same kind of
 *     position floats up, through [JuxCompletionStatistician].
 *  5. **Accessibility**, then **deprecation**: an item the caret cannot
 *     reach, and one marked deprecated, sink to the bottom.
 *
 * The `P_*` tiers of [JuxCompletionContributor] stay in the chain as the
 * last resort (`TierWeigher`), so two items the rules above cannot separate
 * keep the order they always had. They are deliberately NOT a
 * `PrioritizedLookupElement` priority any more: the platform weighs that
 * first of all, above even prefix quality, which is what kept the old popup
 * from ever putting the best match on top.
 */
object JuxCompletionRanking {

    /** Where an item comes from, in the order locality ranks it. */
    enum class Kind {
        /** A local variable (nearest declaration first among them). */
        LOCAL,
        /** A parameter of the enclosing method or lambda. */
        PARAM,
        /** A member of the enclosing class, or of the receiver's own class after `.`. */
        MEMBER,
        /** A member inherited from a supertype. */
        INHERITED,
        /** A member every value has (`operator string()`, `operator hash()`). */
        UNIVERSAL,
        /** A free function of the file. */
        TOP_LEVEL,
        /** A keyword. */
        KEYWORD,
        /** A type declared in this file (or an enclosing type parameter). */
        TYPE_FILE,
        /**
         * A type in this file's own package, declared in another file. It
         * needs no `import` (§4.4), so it is as reachable as one written
         * here, and ranks directly below what the file itself declares.
         */
        TYPE_PACKAGE,
        /**
         * A built-in type name: the primitives, `String` and `string`. Always
         * in scope and never imported, but a name the user declared is more
         * likely to be the one they are reaching for, so they sit below the
         * file's and the package's own types.
         */
        TYPE_PRIMITIVE,
        /** A type this file already imports: the user has already chosen it once. */
        TYPE_IMPORTED,
        /**
         * A type the auto-prelude binds with no declaration and no `import`
         * (`Vec`, `HashMap`, `Option`, the exception hierarchy). Reachable
         * everywhere, but not something this file has committed to.
         */
        TYPE_PRELUDE,
        /** A type from another file of the project, which accepting would import. */
        TYPE_PROJECT,
        /** A type from a dependency package or a generated stub, which accepting would import. */
        TYPE_LIBRARY,
        /** Anything else. */
        OTHER,
    }

    /** An item's kind, recorded by the contributor when it creates the item. */
    val KIND: Key<Kind> = Key.create("jux.completion.kind")

    /** The old `P_*` relevance tier, the sorter's last resort. */
    val TIER: Key<Double> = Key.create("jux.completion.tier")

    /** An item's type as the caret sees it (a method's return type), when known up front. */
    val TYPE: Key<JuxType> = Key.create("jux.completion.type")

    /** Set on an item the caret cannot legally reach (it is sunk, not dropped). */
    val INACCESSIBLE: Key<Boolean> = Key.create("jux.completion.inaccessible")

    /**
     * Expected types per completion session. The sorter, smart completion and
     * the statistician all ask, so it is computed once; keyed weakly by the
     * session's parameters (identity), never stored on PSI that outlives it.
     */
    private val expectedCache: MutableMap<CompletionParameters, List<JuxType>> =
        java.util.Collections.synchronizedMap(java.util.WeakHashMap())

    /** The sorter for one completion session at [parameters]. */
    fun sorter(parameters: CompletionParameters, matcher: PrefixMatcher): CompletionSorter {
        val expected = expectedTypes(parameters)
        val caretType = PsiTreeUtil.getParentOfType(parameters.position, JuxTypeDeclaration::class.java)
        return CompletionSorter.defaultSorter(parameters, matcher)
            .weighAfter("prefix", ExpectedTypeWeigher(expected), LocalityWeigher(caretType))
            .weighAfter("stats", AccessibilityWeigher, DeprecationWeigher, TierWeigher)
    }

    // ------------------------------------------------------------ item facts

    /** The element under any decorators ([com.intellij.codeInsight.completion.PrioritizedLookupElement] and friends). */
    fun unwrap(element: LookupElement): LookupElement {
        var e = element
        while (e is LookupElementDecorator<*>) e = e.delegate
        return e
    }

    /** Read [key] from [element] or any element it decorates. */
    fun <T> dataOf(element: LookupElement, key: Key<T>): T? {
        var e: LookupElement = element
        while (true) {
            e.getUserData(key)?.let { return it }
            e = (e as? LookupElementDecorator<*>)?.delegate ?: return null
        }
    }

    /** The declaration an item stands for, when it stands for one. */
    fun declarationOf(element: LookupElement): PsiElement? = unwrap(element).psiElement

    /** The type of the value [element] would insert, or [JuxType.Unknown]. */
    fun typeOf(element: LookupElement): JuxType {
        dataOf(element, TYPE)?.let { return it }
        val decl = declarationOf(element) ?: return keywordType(element.lookupString)
        return when {
            decl is JuxTypeDeclaration -> JuxType.Static(decl)
            decl.elementType === E.METHOD_DECLARATION -> {
                val owner = PsiTreeUtil.getParentOfType(decl, JuxTypeDeclaration::class.java)
                if (owner != null) JuxTypeEngine.returnType(JuxMember(decl, JuxTypeEngine.selfType(owner)))
                else JuxTypeEngine.declaredType(decl)
            }
            else -> JuxTypeEngine.declaredType(decl)
        }
    }

    private fun keywordType(word: String): JuxType = when (word) {
        "true", "false" -> JuxType.Primitive("bool")
        "null" -> JuxType.Nullable(JuxType.Unknown)
        else -> JuxType.Unknown
    }

    /** An item's kind: what the contributor recorded, refined from the declaration where it can be. */
    fun kindOf(element: LookupElement, caretType: JuxTypeDeclaration?): Kind {
        dataOf(element, KIND)?.let { return it }
        val decl = declarationOf(element)
        if (decl != null) {
            if (decl.elementType === E.METHOD_DECLARATION && decl.parent is JuxFile) return Kind.TOP_LEVEL
            val owner = PsiTreeUtil.getParentOfType(decl, JuxTypeDeclaration::class.java)
            if (owner != null && decl !is JuxTypeDeclaration) {
                return if (caretType == null || owner == caretType || PsiTreeUtil.isAncestor(owner, caretType, false)) Kind.MEMBER
                else Kind.INHERITED
            }
        }
        return Kind.OTHER
    }

    /** True when [decl] lives in a library (a dependency or a generated stub), not the project's own code. */
    fun isLibrary(decl: PsiElement): Boolean {
        val vf = decl.containingFile?.originalFile?.virtualFile ?: return false
        if (vf.name.endsWith(".jux.d")) return true
        return ProjectFileIndex.getInstance(decl.project).isInLibrary(vf)
    }

    // ------------------------------------------------------------ expected type

    /**
     * The types the caret position wants, most specific first; empty when the
     * position does not constrain the type (a statement start, a free
     * expression).
     */
    fun expectedTypes(parameters: CompletionParameters): List<JuxType> {
        expectedCache[parameters]?.let { return it }
        val computed = try {
            computeExpected(parameters.position)
        } catch (e: com.intellij.openapi.progress.ProcessCanceledException) {
            throw e
        } catch (_: Exception) {
            emptyList()
        }
        expectedCache[parameters] = computed
        return computed
    }

    private fun computeExpected(position: PsiElement): List<JuxType> {
        // The expression being completed: the reference at the caret, or the
        // member access `recv.<caret>` it is the name of.
        // `a.b|` parses the name as a leaf of the field access itself, so the
        // leaf's parent is the whole expression either way.
        var expr: PsiElement = position.parent ?: return emptyList()
        // `new Fo|` completes the type of a NEW_EXPRESSION.
        if (expr.elementType === E.TYPE_REFERENCE && expr.parent?.elementType === E.NEW_EXPRESSION) expr = expr.parent
        if (expr.elementType !== E.REFERENCE_EXPRESSION && expr.elementType !== E.FIELD_ACCESS_EXPRESSION &&
            expr.elementType !== E.NEW_EXPRESSION
        ) return emptyList()
        while (expr.parent?.elementType === E.PARENTHESIZED_EXPRESSION) expr = expr.parent
        val parent = expr.parent ?: return emptyList()
        return when (parent.elementType) {
            E.LOCAL_VARIABLE, E.FIELD_DECLARATION, E.PROPERTY_DECLARATION, E.CONST_DECLARATION ->
                if (isAfterEq(parent, expr)) declaredTypeOf(parent) else emptyList()
            E.ASSIGNMENT_EXPRESSION ->
                if (JuxTypeEngine.firstExpressionChild(parent) != expr) listOfKnown(JuxTypeEngine.typeOf(JuxTypeEngine.firstExpressionChild(parent)))
                else emptyList()
            E.RETURN_STATEMENT -> returnTypeAt(parent)
            E.IF_STATEMENT, E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT -> listOf(JuxType.Primitive("bool"))
            E.CONDITIONAL_EXPRESSION ->
                if (JuxTypeEngine.firstExpressionChild(parent) == expr) listOf(JuxType.Primitive("bool")) else emptyList()
            E.UNARY_EXPRESSION ->
                if (parent.firstChild?.elementType === T.BANG) listOf(JuxType.Primitive("bool")) else emptyList()
            E.BINARY_EXPRESSION -> binaryOperandExpectation(parent, expr)
            E.ARGUMENT_LIST -> argumentExpectation(parent, expr)
            else -> emptyList()
        }
    }

    private fun listOfKnown(t: JuxType): List<JuxType> = if (t is JuxType.Unknown) emptyList() else listOf(t)

    private fun isAfterEq(decl: PsiElement, expr: PsiElement): Boolean {
        var c = decl.firstChild
        var sawEq = false
        while (c != null) {
            if (c.elementType === T.EQ) sawEq = true
            if (c == expr) return sawEq
            c = c.nextSibling
        }
        return false
    }

    private fun declaredTypeOf(decl: PsiElement): List<JuxType> {
        val ref = decl.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return emptyList()
        return listOfKnown(JuxTypeEngine.typeOfTypeReference(ref))
    }

    private fun returnTypeAt(ret: PsiElement): List<JuxType> {
        var p: PsiElement? = ret.parent
        while (p != null && p !is JuxFile) {
            when (p.elementType) {
                E.LAMBDA_EXPRESSION -> return emptyList()
                E.METHOD_DECLARATION, E.OPERATOR_DECLARATION -> return declaredTypeOf(p)
            }
            p = p.parent
        }
        return emptyList()
    }

    private fun binaryOperandExpectation(binary: PsiElement, expr: PsiElement): List<JuxType> {
        val operands = JuxTypeEngine.expressionChildren(binary)
        val op = binary.node.getChildren(null).firstOrNull {
            it.psi !in operands && it.elementType != com.intellij.psi.TokenType.WHITE_SPACE && it.elementType !in T.COMMENTS
        }?.elementType
        return when (op) {
            T.AND_AND, T.OR_OR -> listOf(JuxType.Primitive("bool"))
            T.EQ_EQ, T.NOT_EQ -> {
                val other = operands.firstOrNull { it != expr } ?: return emptyList()
                listOfKnown(JuxTypeEngine.typeOf(other))
            }
            else -> emptyList()
        }
    }

    private fun argumentExpectation(args: PsiElement, expr: PsiElement): List<JuxType> {
        val index = JuxTypeEngine.expressionChildren(args).indexOf(expr).takeIf { it >= 0 } ?: return emptyList()
        val call = args.parent ?: return emptyList()
        if (call.elementType !== E.CALL_EXPRESSION) return emptyList()
        val callee = call.firstChild ?: return emptyList()
        val argCount = JuxTypeEngine.argumentCount(call)
        val (method, owner) = when (callee.elementType) {
            E.FIELD_ACCESS_EXPRESSION -> {
                val m = JuxTypeEngine.resolveMemberAccess(callee, argCount) ?: return emptyList()
                m.element to m.owner
            }
            E.REFERENCE_EXPRESSION -> {
                val target = JuxTypeEngine.resolveReferenceExpression(callee, argCount) ?: return emptyList()
                target to PsiTreeUtil.getParentOfType(target, JuxTypeDeclaration::class.java)?.let { JuxTypeEngine.selfType(it) }
            }
            else -> return emptyList()
        }
        if (method.elementType !== E.METHOD_DECLARATION) return emptyList()
        val params = method.node.findChildByType(E.PARAMETER_LIST)?.psi?.children
            ?.filter { it.elementType === E.PARAMETER } ?: return emptyList()
        val param = params.getOrNull(index) ?: return emptyList()
        val raw = declaredTypeOf(param).firstOrNull() ?: return emptyList()
        val subst = owner?.let { JuxTypeEngine.substitution(it) } ?: emptyMap()
        return listOfKnown(JuxTypeEngine.substitute(raw, subst))
    }

    // ------------------------------------------------------------ fitting

    /**
     * How well [item] fits [expected]: 0 fits, 1 unknown, 2 does not fit.
     * A value fits its own type, any supertype, and the nullable form of
     * either; a narrower number fits a wider one; `null` fits only a nullable.
     */
    fun fit(item: JuxType, expected: JuxType): Int {
        if (item is JuxType.Unknown || expected is JuxType.Unknown) return 1
        if (expected is JuxType.Nullable) {
            if (item is JuxType.Nullable && item.inner is JuxType.Unknown) return 0 // `null`
            return fit(JuxTypeEngine.stripNullable(item), expected.inner)
        }
        // A nullable value (or `null` itself) does not fit a non-null slot.
        if (item is JuxType.Nullable) return 2
        return when (item) {
            is JuxType.Primitive -> primitiveFit(item.name, expected)
            is JuxType.ClassType -> when (expected) {
                is JuxType.ClassType ->
                    if (JuxTypeEngine.typeAndSupertypes(item).any { sameDeclaration(it.decl, expected.decl) }) 0 else 2
                is JuxType.Primitive -> if (item.decl.name == expected.name) 0 else 2
                is JuxType.TypeVar -> expected.bound?.let { fit(item, it) } ?: 1
                else -> 2
            }
            is JuxType.ArrayType -> if (expected is JuxType.ArrayType) fit(item.element, expected.element) else 2
            // Function types (Type system §T.3.6): parameters contravariant,
            // result covariant. `(Animal) -> R` fits a `(Dog) -> R` slot and
            // `() -> Dog` fits `() -> Animal`; the reverse directions do not.
            is JuxType.FunctionType -> when (expected) {
                is JuxType.FunctionType -> {
                    if (item.params.size != expected.params.size) 2
                    else {
                        val parts = item.params.zip(expected.params).map { (have, want) -> fit(want, have) } +
                            fit(item.ret, expected.ret).let { if (expected.ret == JuxType.Primitive("void")) 0 else it }
                        parts.maxOrNull() ?: 0
                    }
                }
                else -> 2
            }
            is JuxType.TypeVar -> if (expected is JuxType.TypeVar && expected.param == item.param) 0 else 1
            // A type name is the way to a value of it (`new Point(..)`,
            // `Color.RED`), so a type fitting the slot ranks with the values
            // that fit, as Java ranks the expected class.
            is JuxType.Static -> fit(JuxTypeEngine.selfType(item.decl), expected)
            else -> 1
        }
    }

    private fun primitiveFit(name: String, expected: JuxType): Int {
        val exp = when (expected) {
            is JuxType.Primitive -> expected.name
            is JuxType.ClassType -> expected.decl.name ?: return 2
            is JuxType.TypeVar -> return 1
            else -> return 2
        }
        if (name.equals(exp, ignoreCase = name == "String" || name == "string")) return 0
        val widen = NUMERIC_WIDENING[name] ?: return 2
        return if (exp in widen) 0 else 2
    }

    /** Which numeric types a value of each type widens into without a cast. */
    private val NUMERIC_WIDENING: Map<String, Set<String>> = mapOf(
        "byte" to setOf("short", "int", "long", "float", "double"),
        "short" to setOf("int", "long", "float", "double"),
        "char" to setOf("int", "long", "float", "double"),
        "int" to setOf("long", "float", "double"),
        "long" to setOf("float", "double"),
        "float" to setOf("double"),
    )

    /**
     * The same declaration, seen from either side of completion's file copy:
     * the caret's expected type is read in the copy, while items carry the
     * original declarations.
     */
    private fun sameDeclaration(a: PsiElement, b: PsiElement): Boolean =
        a == b || CompletionUtil.getOriginalOrSelf(a) == CompletionUtil.getOriginalOrSelf(b)

    /** The best fit of [item] against any of [expected] (1 when nothing is expected). */
    fun bestFit(item: JuxType, expected: List<JuxType>): Int =
        if (expected.isEmpty()) 1 else expected.minOf { fit(item, it) }

    // ------------------------------------------------------------ weighers

    private class ExpectedTypeWeigher(private val expected: List<JuxType>) : LookupElementWeigher("juxExpectedType") {
        // Fitting items first; everything else (a mismatch, a keyword, an
        // unknown type) stays together so locality orders it, as Java's
        // expected-type weigher does.
        override fun weigh(element: LookupElement): Comparable<*> =
            if (expected.isEmpty() || bestFit(typeOf(element), expected) == 0) 0 else 1
    }

    private class LocalityWeigher(private val caretType: JuxTypeDeclaration?) : LookupElementWeigher("juxLocality") {
        override fun weigh(element: LookupElement): Comparable<*> {
            val kind = kindOf(element, caretType)
            // Among locals, the nearest declaration (the latest before the caret) first.
            val nearness = if (kind == Kind.LOCAL) -(declarationOf(element)?.textOffset ?: 0) else 0
            return Pair(kind.ordinal, nearness).let { (a, b) -> a.toLong() * 100_000_000L + b }
        }
    }

    /** The contributor's old `P_*` tier: whatever the rules above left tied keeps its former order. */
    private object TierWeigher : LookupElementWeigher("juxTier") {
        override fun weigh(element: LookupElement): Comparable<*> = -(dataOf(element, TIER) ?: 0.0)
    }

    private object AccessibilityWeigher : LookupElementWeigher("juxAccessibility") {
        override fun weigh(element: LookupElement): Comparable<*> = if (dataOf(element, INACCESSIBLE) == true) 1 else 0
    }

    private object DeprecationWeigher : LookupElementWeigher("juxDeprecation") {
        override fun weigh(element: LookupElement): Comparable<*> {
            val decl = declarationOf(element) ?: return 0
            return if (isDeprecated(decl)) 1 else 0
        }
    }

    /**
     * Whether [decl] carries `@Deprecated`. Annotations sit either directly on
     * the declaration or inside its modifier list; names compare
     * case-insensitively, as Jux annotation names do.
     */
    fun isDeprecated(decl: PsiElement): Boolean {
        val holders = listOfNotNull(decl, decl.node.findChildByType(E.MODIFIER_LIST)?.psi)
        return holders.any { holder ->
            holder.children.any {
                it.elementType === E.ANNOTATION &&
                    it.text.removePrefix("@").substringBefore('(').trim().equals("deprecated", ignoreCase = true)
            }
        }
    }
}

/**
 * Usage statistics for Jux completion, as Java keeps them: what the user
 * picks is remembered per kind of position (the type the caret wanted), and
 * the platform's `stats` weigher floats it up next time.
 */
class JuxCompletionStatistician : CompletionStatistician() {
    override fun serialize(element: LookupElement, location: CompletionLocation): StatisticsInfo? {
        val params = location.completionParameters
        if (params.originalFile !is JuxFile) return null
        val expected = JuxCompletionRanking.expectedTypes(params)
        val context = "jux#" + (expected.firstOrNull()?.presentable() ?: "any")
        return StatisticsInfo(context, element.lookupString)
    }
}
