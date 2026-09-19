package dev.jux.intellij.inspections

import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.editor.JuxImportSupport
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * How a variable is used at one place: read, written, or both.
 *
 * The final-candidate and never-read inspections are the Java data-flow
 * checks that need no data-flow engine: they only ask, for each use of a
 * variable, whether that use stores into it. Everything they do not
 * understand counts as both a read and a write, which only ever makes them
 * say less.
 *
 * - `x = e` (the left side of a plain `=`) is a pure write;
 * - `x += e`, `x++`, `--x` read and write;
 * - an argument written `out x` or `ref x` reads and writes (the callee
 *   stores through it);
 * - everything else is a read.
 */
object JuxAccess {

    enum class Kind { READ, WRITE, READ_WRITE }

    /** What the use [ref] (a name or a `this.x` / `obj.x` access) does to its variable. */
    fun kindOf(ref: PsiElement): Kind {
        var use = ref
        // `(x) = 1` and `((x))++` store into x just the same.
        while (use.parent?.elementType === E.PARENTHESIZED_EXPRESSION) use = use.parent
        val parent = use.parent ?: return Kind.READ
        when (parent.elementType) {
            E.ASSIGNMENT_EXPRESSION -> {
                if (JuxTypeEngine.expressionChildren(parent).firstOrNull() !== use) return Kind.READ
                val op = JuxCodeFacts.binaryOperator(parent)
                return if (op?.elementType === T.EQ) Kind.WRITE else Kind.READ_WRITE
            }
            E.UNARY_EXPRESSION, E.POSTFIX_EXPRESSION ->
                if (parent.node.findChildByType(T.PLUS_PLUS) != null || parent.node.findChildByType(T.MINUS_MINUS) != null) {
                    return Kind.READ_WRITE
                }
        }
        val before = PsiTreeUtil.prevVisibleLeaf(use)
        if (before != null && parent.elementType === E.ARGUMENT_LIST &&
            (before.elementType === T.REF_KW || (before.elementType === T.IDENTIFIER && before.text == "out"))
        ) return Kind.READ_WRITE
        return Kind.READ
    }

    /**
     * Every use of every variable in [file], by declaration, plus the names of
     * uses the resolver could not place. A declaration whose name is among
     * [Census.unresolved] may have uses nobody counted, so the inspections
     * leave it alone.
     */
    class Census(val uses: Map<PsiElement, List<PsiElement>>, val unresolved: Set<String>)

    fun census(file: PsiFile): Census {
        val uses = HashMap<PsiElement, MutableList<PsiElement>>()
        val unresolved = HashSet<String>()
        PsiTreeUtil.processElements(file) { e ->
            when (e.elementType) {
                E.REFERENCE_EXPRESSION -> {
                    val target = runCatching { JuxTypeEngine.resolveReferenceExpression(e) }.getOrNull()
                    if (target != null) uses.getOrPut(target) { ArrayList() }.add(e)
                    else nameOf(e)?.let(unresolved::add)
                }
                E.FIELD_ACCESS_EXPRESSION -> {
                    val target = runCatching { JuxTypeEngine.resolveMemberAccess(e)?.element }.getOrNull()
                    if (target != null) uses.getOrPut(target) { ArrayList() }.add(e)
                    else nameOf(e)?.let(unresolved::add)
                }
            }
            true
        }
        return Census(uses, unresolved)
    }

    /** The name a use mentions: the last identifier of `x` or `a.b.x`. */
    fun nameOf(use: PsiElement): String? {
        var c: PsiElement? = use.lastChild
        while (c != null && c.elementType !== T.IDENTIFIER) c = c.prevSibling
        return c?.text
    }

    /** True when [kind] stores into the variable. */
    fun writes(kind: Kind) = kind != Kind.READ

    /** True when [kind] reads the variable. */
    fun reads(kind: Kind) = kind != Kind.WRITE

    /**
     * Names the resolver cannot see: interpolation holes (`$"${count}"` is one
     * token), raw identifiers inside patterns, annotation arguments, `new`
     * expressions and where-clauses. A variable mentioned there may be read,
     * so no inspection here may call it unread.
     */
    fun blindMentions(file: PsiFile): Set<String> {
        val out = HashSet<String>()
        PsiTreeUtil.processElements(file) { e ->
            when (e.elementType) {
                T.INTERP_STRING_LITERAL -> out.addAll(JuxImportSupport.interpolatedNames(e.text, raw = false))
                T.INTERP_RAW_STRING_LITERAL -> out.addAll(JuxImportSupport.interpolatedNames(e.text, raw = true))
                T.IDENTIFIER -> if (e.parent?.elementType in BLIND_PARENTS) out.add(e.text)
                else -> {}
            }
            true
        }
        return out
    }

    private val BLIND_PARENTS = setOf(E.PATTERN, E.NEW_EXPRESSION, E.ANNOTATION, E.WHERE_CLAUSE)

    /** The modifier keywords written on [decl] (`private`, `final`, ...). */
    fun modifiers(decl: PsiElement): Set<String> {
        val list = decl.node.findChildByType(E.MODIFIER_LIST)?.psi
        val words = HashSet<String>()
        list?.let { PsiTreeUtil.collectElements(it) { leaf -> leaf.firstChild == null }.forEach { leaf -> words.add(leaf.text) } }
        // A local's `final` sits directly on the declaration, with no list.
        var c: PsiElement? = decl.firstChild
        while (c != null && c.elementType !== T.IDENTIFIER && c.elementType !== E.TYPE_REFERENCE) {
            if (c.elementType === T.FINAL_KW || c.elementType === T.CONST_KW) words.add(c.text)
            c = c.nextSibling
        }
        return words
    }

    /** The initializer of a local or field (the expression after its `=`), or null. */
    fun initializerOf(decl: PsiElement): PsiElement? {
        var sawEq = false
        var c: PsiElement? = decl.firstChild
        while (c != null) {
            if (c.elementType === T.EQ) sawEq = true
            else if (sawEq && JuxTypeEngine.isExpression(c)) return c
            c = c.nextSibling
        }
        return null
    }
}
