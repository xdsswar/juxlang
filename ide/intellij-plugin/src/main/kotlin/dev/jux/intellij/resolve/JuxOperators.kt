package dev.jux.intellij.resolve

import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.tree.IElementType
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * User operators as the editor sees them: which `operator` declaration an
 * expression like `a * 2.0`, `start..end` or `price += 1L` calls.
 *
 * The compiler picks among a type's operators the way it picks a method
 * overload (JUX-OPERATORS-ADDENDUM §O.2.3): the left operand's type and its
 * supertypes supply the candidates, nearest declaration first, and the right
 * operand's type chooses among the ones with the same symbol. A compound
 * assignment (`v *= k`) uses the operator it is built from. This object does
 * the same over PSI so Ctrl+B, Find Usages, highlighting of usages and the
 * expression's inferred type all land on the member the compiler would call.
 *
 * Only binary operators are resolved: they are the ones a type can declare
 * several of (one per operand type), and the ones an interface can require
 * (§7.14.6). Equality, ordering and `string` are single per type and are
 * reached by name elsewhere.
 */
object JuxOperators {

    /** Compound assignment tokens and the binary operator each applies. */
    private val COMPOUND: Map<IElementType, String> = mapOf(
        T.PLUS_EQ to "+", T.MINUS_EQ to "-", T.STAR_EQ to "*", T.SLASH_EQ to "/",
        T.PERCENT_EQ to "%", T.AMP_EQ to "&", T.PIPE_EQ to "|", T.CARET_EQ to "^",
        T.LT_LT_EQ to "<<", T.GT_GT_EQ to ">>",
    )

    /**
     * Binary operator tokens a user type may declare. Comparisons, `&&`/`||`,
     * the type test and `as` are left out: they are never user overloads that
     * choose by operand type.
     */
    private val OVERLOADABLE: Set<IElementType> = setOf(
        T.PLUS, T.MINUS, T.STAR, T.SLASH, T.PERCENT, T.AMP, T.PIPE, T.CARET,
        T.LT_LT, T.GT_GT, T.DOT_DOT, T.DOT_DOT_EQ,
        T.PLUS_PERCENT, T.MINUS_PERCENT, T.STAR_PERCENT, T.LT_LT_PERCENT, T.GT_GT_PERCENT,
    )

    /** Primitive numeric widening ranks: a value widens to a type of higher rank in its family. */
    private val INTEGER_RANK = mapOf("byte" to 1, "short" to 2, "int" to 3, "long" to 4)
    private val UNSIGNED_RANK = mapOf("ubyte" to 1, "ushort" to 2, "uint" to 3, "ulong" to 4)
    private val FLOAT_RANK = mapOf("float" to 5, "double" to 6)

    // ------------------------------------------------------------ declarations

    /**
     * The symbol an `operator` declaration defines: `*` of
     * `Vec2 operator*(double k)`, `..=` of `operator..=(Date end)`, `string`
     * of `operator string()`. Null when [decl] is not an operator.
     */
    fun symbolOf(decl: PsiElement): String? {
        if (decl.elementType !== E.OPERATOR_DECLARATION) return null
        val sb = StringBuilder()
        var sawKeyword = false
        var c: PsiElement? = decl.firstChild
        while (c != null) {
            val t = c.elementType
            if (t === T.OPERATOR_KW) {
                sawKeyword = true
            } else if (sawKeyword) {
                if (t === E.PARAMETER_LIST || t === E.CODE_BLOCK || t === T.SEMICOLON || t === T.LBRACE) break
                if (c !is PsiWhiteSpace) sb.append(c.text)
            }
            c = c.nextSibling
        }
        return sb.toString().takeIf { sawKeyword && it.isNotEmpty() }
    }

    /**
     * The leaf a marker or a navigation target should sit on: the `operator`
     * keyword (an operator usually has no name identifier to point at).
     */
    fun anchorOf(decl: PsiElement): PsiElement? = decl.node.findChildByType(T.OPERATOR_KW)?.psi

    /** Every operator declared directly in [type] with [symbol] and [arity] parameters. */
    fun declaredIn(type: JuxTypeDeclaration, symbol: String, arity: Int = 1): List<PsiElement> =
        JuxHierarchy.directChildren(type, E.OPERATOR_DECLARATION)
            .filter { symbolOf(it) == symbol && JuxHierarchy.arity(it) == arity }

    // ------------------------------------------------------------------- uses

    /**
     * The operator token of a use: the `*` of `a * b`, the `..` of `a..b`, the
     * `*=` of `v *= k`. Null for anything that is not a user-overloadable
     * binary use.
     */
    fun operatorTokenOf(use: PsiElement): PsiElement? {
        if (use.elementType !== E.BINARY_EXPRESSION && use.elementType !== E.RANGE_EXPRESSION &&
            use.elementType !== E.ASSIGNMENT_EXPRESSION
        ) return null
        var c: PsiElement? = use.firstChild
        while (c != null) {
            val t = c.elementType
            if (t != null && (t in OVERLOADABLE || t in COMPOUND.keys)) return c
            c = c.nextSibling
        }
        return null
    }

    /** The symbol a use calls: the token's text, or the base operator of a compound assignment. */
    fun symbolOfUse(token: PsiElement): String? {
        val t = token.elementType ?: return null
        return COMPOUND[t] ?: if (t in OVERLOADABLE) token.text else null
    }

    /**
     * The operator declaration [use] calls, with the type it was found
     * through (for generic substitution of its return type), or null when the
     * left operand has no user operator for the symbol (a primitive `+`, an
     * unresolved type).
     */
    fun resolve(use: PsiElement): JuxMember? {
        val token = operatorTokenOf(use) ?: return null
        val symbol = symbolOfUse(token) ?: return null
        val operands = JuxTypeEngine.expressionChildren(use)
        val left = operands.getOrNull(0) ?: return null
        val right = operands.getOrNull(1)
        val leftType = JuxTypeEngine.typeOf(left)
        val rightType = right?.let { JuxTypeEngine.typeOf(it) } ?: JuxType.Unknown

        val candidates = ArrayList<JuxMember>()
        JuxTypeEngine.classOf(leftType)?.let { ct ->
            for (owner in JuxTypeEngine.typeAndSupertypes(ct)) {
                for (op in declaredIn(owner.decl, symbol)) candidates.add(JuxMember(op, owner))
            }
        }
        if (candidates.isEmpty()) return freeOperator(use, symbol, leftType, rightType)
        return pick(candidates, rightType)
    }

    /**
     * A free (top-level) operator of the use's own file, `Money operator+(Money a,
     * long b)`: chosen by both operand types. Free operators elsewhere are
     * imported by name and are found through the type's own set first.
     */
    private fun freeOperator(use: PsiElement, symbol: String, left: JuxType, right: JuxType): JuxMember? {
        val file = use.containingFile ?: return null
        val leftClass = JuxTypeEngine.classOf(left) ?: return null
        val matches = topLevelOperators(file).filter { op ->
            symbolOf(op) == symbol && JuxHierarchy.arity(op) == 2 &&
                score(JuxTypeEngine.declaredType(JuxHierarchy.parameters(op)[0]), left) > 0
        }
        val best = matches.maxByOrNull { op -> score(JuxTypeEngine.declaredType(JuxHierarchy.parameters(op)[1]), right) }
            ?: return null
        return JuxMember(best, leftClass)
    }

    private fun topLevelOperators(file: PsiFile): List<PsiElement> =
        file.children.filter { it.elementType === E.OPERATOR_DECLARATION }

    /**
     * The candidate whose single parameter fits [rightType] best. A candidate
     * that cannot take the operand at all is only chosen when it is the only
     * one, so a half-typed expression still navigates somewhere sensible.
     */
    private fun pick(candidates: List<JuxMember>, rightType: JuxType): JuxMember? {
        var best: JuxMember? = null
        var bestScore = -1
        for (c in candidates) {
            val param = JuxHierarchy.parameters(c.element).firstOrNull() ?: continue
            val paramType = JuxTypeEngine.substitute(
                JuxTypeEngine.declaredType(param), JuxTypeEngine.substitution(c.owner),
            )
            val s = score(paramType, rightType)
            if (s > bestScore) { best = c; bestScore = s }
        }
        return if (bestScore > 0 || candidates.size == 1) best else null
    }

    /**
     * How well an operand of type [arg] fits a parameter of type [param]:
     * 4 the same type, 3 a subtype, 2 a primitive widening, 1 unknown on
     * either side (still acceptable), 0 no fit.
     */
    fun score(param: JuxType, arg: JuxType): Int {
        val p = JuxTypeEngine.stripNullable(param)
        val a = JuxTypeEngine.stripNullable(arg)
        if (p is JuxType.Unknown || a is JuxType.Unknown || p is JuxType.TypeVar) return 1
        if (p.presentable() == a.presentable()) return 4
        if (p is JuxType.ClassType && a is JuxType.ClassType) {
            return if (JuxTypeEngine.typeAndSupertypes(a).any { it.decl == p.decl }) 3 else 0
        }
        if (p is JuxType.Primitive && a is JuxType.Primitive) {
            return if (widens(a.name, p.name)) 2 else 0
        }
        return 0
    }

    /** Whether a primitive [from] widens implicitly to [to] (Jux's numeric promotion). */
    private fun widens(from: String, to: String): Boolean {
        INTEGER_RANK[from]?.let { f ->
            INTEGER_RANK[to]?.let { return it > f }
            return to in FLOAT_RANK
        }
        UNSIGNED_RANK[from]?.let { f ->
            UNSIGNED_RANK[to]?.let { return it > f }
            return to in FLOAT_RANK
        }
        FLOAT_RANK[from]?.let { f -> FLOAT_RANK[to]?.let { return it > f } }
        return false
    }

    // ------------------------------------------------------ interface contracts

    /**
     * The interface operator [decl] fills: for `Money operator+(Money other)`
     * in a class implementing `Addable<Money>`, the bodiless `T operator+(T
     * other);` of `Addable`. Null when no supertype declares the symbol.
     */
    fun findSuperOperator(decl: PsiElement): PsiElement? {
        val symbol = symbolOf(decl) ?: return null
        val owner = JuxHierarchy.enclosingType(decl) ?: return null
        val arity = JuxHierarchy.arity(decl)
        val self = JuxTypeEngine.selfType(owner)
        val mine = JuxHierarchy.parameters(decl).firstOrNull()?.let { JuxTypeEngine.declaredType(it) }
        for (superType in JuxTypeEngine.typeAndSupertypes(self).drop(1)) {
            for (op in declaredIn(superType.decl, symbol, arity)) {
                if (mine == null) return op
                val theirs = JuxHierarchy.parameters(op).firstOrNull() ?: return op
                val theirType = JuxTypeEngine.substitute(
                    JuxTypeEngine.declaredType(theirs), JuxTypeEngine.substitution(superType),
                )
                if (score(theirType, mine) >= 3 || score(theirType, mine) == 1) return op
            }
        }
        return null
    }

    /**
     * The class operators that fill an interface operator [decl], found among
     * [subtypes] (the caller supplies them, from the project-wide subtype search).
     */
    fun implementationsOf(decl: PsiElement, subtypes: List<JuxTypeDeclaration>): List<PsiElement> {
        val symbol = symbolOf(decl) ?: return emptyList()
        val arity = JuxHierarchy.arity(decl)
        return subtypes.flatMap { declaredIn(it, symbol, arity) }.filter { findSuperOperator(it) == decl }
    }
}
