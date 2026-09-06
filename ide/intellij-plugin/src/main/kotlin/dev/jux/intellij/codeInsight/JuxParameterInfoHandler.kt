package dev.jux.intellij.codeInsight

import com.intellij.lang.parameterInfo.CreateParameterInfoContext
import com.intellij.lang.parameterInfo.ParameterInfoHandler
import com.intellij.lang.parameterInfo.ParameterInfoUIContext
import com.intellij.lang.parameterInfo.UpdateParameterInfoContext
import com.intellij.psi.PsiElement
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeIndex
import dev.jux.intellij.resolve.JuxTypeInference

/**
 * The Parameter Info popup (`Ctrl+P`) — the signature of the call the caret is
 * inside, with the parameter being typed shown in bold.
 *
 * Jux overloads on parameter *types*, not just arity, so every candidate with a
 * matching name is offered and the platform pages between them with the arrows.
 * That is also why this cannot reuse [JuxHierarchy.allMembers], which dedupes
 * methods by name and arity and would silently drop `print(int)` the moment
 * `print(String)` existed.
 *
 * The active-parameter rule — count depth-0 commas before the caret — is
 * deliberately the same one `juxc-lsp` applies in `find_enclosing_call`, so the
 * popup and the server's signature help cannot disagree about which parameter
 * you are on.
 */
class JuxParameterInfoHandler : ParameterInfoHandler<PsiElement, JuxParameterInfoHandler.Signature> {

    /** One candidate signature: the rendered parameter list of one overload. */
    data class Signature(val params: List<String>, val label: String)

    override fun findElementForParameterInfo(context: CreateParameterInfoContext): PsiElement? {
        val args = argumentListAt(context.file?.findElementAt(context.offset), context.offset)
            ?: return null
        val signatures = signaturesFor(args)
        if (signatures.isEmpty()) return null
        context.itemsToShow = signatures.toTypedArray()
        return args
    }

    override fun showParameterInfo(element: PsiElement, context: CreateParameterInfoContext) {
        context.showHint(element, element.textRange.startOffset + 1, this)
    }

    override fun findElementForUpdatingParameterInfo(context: UpdateParameterInfoContext): PsiElement? =
        argumentListAt(context.file?.findElementAt(context.offset), context.offset)

    override fun updateParameterInfo(parameterOwner: PsiElement, context: UpdateParameterInfoContext) {
        context.setCurrentParameter(activeParameter(parameterOwner, context.offset))
    }

    override fun updateUI(p: Signature?, context: ParameterInfoUIContext) {
        if (p == null) {
            context.isUIComponentEnabled = false
            return
        }
        // An empty list still needs a body, or the popup renders as a blank box.
        if (p.params.isEmpty()) {
            context.setupUIComponentPresentation(
                NO_PARAMETERS, -1, -1, false, false, false, context.defaultParameterColor,
            )
            return
        }
        // The highlight is a range over the joined text, so walk the same join
        // the label was built with rather than recomputing offsets from widths.
        val current = context.currentParameterIndex
        var start = -1
        var end = -1
        var at = 0
        p.params.forEachIndexed { i, text ->
            if (i == current) {
                start = at
                end = at + text.length
            }
            at += text.length + SEPARATOR.length
        }
        context.setupUIComponentPresentation(
            p.label, start, end, false, false, false, context.defaultParameterColor,
        )
    }

    // ---- callee resolution -------------------------------------------------

    /**
     * The ARGUMENT_LIST the caret sits inside, or null when it is not in a call.
     *
     * Both [offset] and `offset - 1` are probed: with the caret immediately
     * after `f(`, `findElementAt` returns the token *after* the paren, which
     * outside a call is the wrong side of the boundary.
     */
    private fun argumentListAt(element: PsiElement?, offset: Int): PsiElement? {
        var e = element
        while (e != null) {
            if (e.node?.elementType === E.ARGUMENT_LIST && offset > e.textRange.startOffset) return e
            e = e.parent
        }
        return null
    }

    /** Number of depth-0 commas between the opening paren and [offset]. */
    private fun activeParameter(args: PsiElement, offset: Int): Int {
        val range = args.textRange
        if (offset <= range.startOffset || offset > range.endOffset) return -1
        val text = args.text
        val upTo = (offset - range.startOffset).coerceIn(0, text.length)
        var depth = 0
        var index = 0
        for (i in 0 until upTo) {
            when (text[i]) {
                '(', '[', '{', '<' -> depth++
                ')', ']', '}', '>' -> depth--
                ',' -> if (depth == 1) index++
            }
        }
        return index
    }

    /**
     * Every declaration the call could be reaching, rendered.
     *
     * Three shapes, matching the three the parser produces: `new T(…)` (the
     * ARGUMENT_LIST's parent is a NEW_EXPRESSION), `recv.m(…)` and a bare
     * `f(…)` (parent is a CALL_EXPRESSION whose first child is the callee).
     */
    private fun signaturesFor(args: PsiElement): List<Signature> {
        val call = args.parent ?: return emptyList()
        val candidates: List<PsiElement> = when (call.node?.elementType) {
            E.NEW_EXPRESSION -> constructorsOf(call, args)
            E.CALL_EXPRESSION -> calleeTargets(call, args)
            else -> emptyList()
        }
        return candidates.mapNotNull { render(it) }.distinctBy { it.label }
    }

    /** Constructors of the type named by a `new T(…)` expression. */
    private fun constructorsOf(newExpr: PsiElement, args: PsiElement): List<PsiElement> {
        val typeRef = newExpr.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return emptyList()
        val type = JuxTypeIndex.findType(args, JuxHierarchy.bareTypeName(typeRef)) ?: return emptyList()
        return JuxHierarchy.directChildren(type, E.CONSTRUCTOR_DECLARATION)
    }

    /** Declarations a `recv.m(…)` or bare `f(…)` call could be reaching. */
    private fun calleeTargets(call: PsiElement, args: PsiElement): List<PsiElement> {
        val callee = call.firstChild ?: return emptyList()
        return when (callee.node?.elementType) {
            // `recv.m(…)` — the member name is the callee's last identifier.
            E.FIELD_ACCESS_EXPRESSION -> {
                val name = callee.lastChild?.text ?: return emptyList()
                val receiver = callee.firstChild?.text ?: return emptyList()
                val target = JuxTypeInference.resolveReceiverExpression(receiver, args)
                    ?: return emptyList()
                methodsNamed(target.type, name)
            }
            // A bare name: a member of the enclosing type, or a free function.
            else -> {
                val name = callee.text.takeIf { it.isNotBlank() } ?: return emptyList()
                val enclosing = JuxHierarchy.enclosingType(args)
                val members = enclosing?.let { methodsNamed(it, name) } ?: emptyList()
                members.ifEmpty { freeFunctionsNamed(args, name) }
            }
        }
    }

    /** Every method named [name] on [type] or a supertype, overloads included. */
    private fun methodsNamed(type: JuxTypeDeclaration, name: String): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        val queue = ArrayDeque<JuxTypeDeclaration>()
        val seen = HashSet<String>()
        queue.add(type)
        while (queue.isNotEmpty()) {
            val t = queue.removeFirst()
            if (!seen.add(t.name ?: continue)) continue
            for (et in CALLABLE_KINDS) {
                for (m in JuxHierarchy.directChildren(t, et)) {
                    if ((m as? JuxNamedElement)?.name == name) out.add(m)
                }
            }
            for (sn in JuxHierarchy.superTypeNames(t)) {
                JuxTypeIndex.findType(t, sn)?.let { queue.add(it) }
            }
        }
        return out
    }

    /** Top-level functions named [name] in the same file (script-mode `main`, helpers). */
    private fun freeFunctionsNamed(context: PsiElement, name: String): List<PsiElement> =
        context.containingFile?.children
            ?.filter {
                it.node?.elementType === E.METHOD_DECLARATION &&
                    (it as? JuxNamedElement)?.name == name
            }
            ?: emptyList()

    /** Render one declaration's parameter list, or null when it has no list. */
    private fun render(decl: PsiElement): Signature? {
        if (decl.node.findChildByType(E.PARAMETER_LIST) == null) return null
        val params = JuxHierarchy.parameters(decl).map { it.text.replace(WHITESPACE, " ").trim() }
        return Signature(params, params.joinToString(SEPARATOR))
    }

    private companion object {
        /** What the popup shows for `f()` — the platform's own wording. */
        const val NO_PARAMETERS = "<no parameters>"
        const val SEPARATOR = ", "
        val WHITESPACE = Regex("\\s+")

        /**
         * Declaration kinds that carry a parameter list and can be called by
         * name. An operator is included: `a.plus(b)` is a legal spelling of
         * `a + b`, so the popup should describe it like any other method.
         */
        val CALLABLE_KINDS = listOf(E.METHOD_DECLARATION, E.OPERATOR_DECLARATION)
    }
}
