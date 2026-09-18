package dev.jux.intellij.hints

import com.intellij.codeInsight.hints.HintInfo
import com.intellij.codeInsight.hints.InlayInfo
import com.intellij.codeInsight.hints.InlayParameterHintsProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeEngine
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * Java-style **parameter name hints**: inside a resolved `foo(a, b)`,
 * `obj.foo(a, b)` or `new Point(a, b)`, show `paramName:` in front of each
 * argument.
 *
 * The callee is resolved through the plugin's type engine, the one member
 * completion uses, so a method reached through a chain (`a.b().run(x)`), an
 * inherited method, one declared in another file, and one from a library
 * stub all get hints, not only methods of the current file.
 *
 * Jux overloads on parameter types, so several declarations can share a name
 * and an argument count. A hint is shown at a position only when every such
 * candidate names that parameter the same: a label is never wrong. As in
 * Java, no hint repeats what the argument already says: `move(x, y)` into
 * parameters `x` and `y` shows nothing.
 */
class JuxInlayHintsProvider : InlayParameterHintsProvider {
    override fun getParameterHints(element: PsiElement): List<InlayInfo> {
        val type = element.elementType
        if (type !== E.CALL_EXPRESSION && type !== E.NEW_EXPRESSION) return emptyList()
        val argList = generateSequence(element.firstChild) { it.nextSibling }
            .firstOrNull { it.elementType === E.ARGUMENT_LIST } ?: return emptyList()
        val args = JuxTypeEngine.expressionChildren(argList)
        if (args.isEmpty()) return emptyList()

        val candidates = if (type === E.NEW_EXPRESSION) constructors(element, args.size)
        else callees(element, args.size)
        if (candidates.isEmpty()) return emptyList()

        val out = ArrayList<InlayInfo>(args.size)
        for ((i, arg) in args.withIndex()) {
            val names = candidates.map { it.getOrNull(i) }.distinct()
            val name = names.singleOrNull() ?: continue
            if (name.isEmpty() || sameAsArgument(arg, name)) continue
            out.add(InlayInfo(name, arg.textRange.startOffset))
        }
        return out
    }

    override fun getHintInfo(element: PsiElement): HintInfo? = null

    override fun getDefaultBlackList(): Set<String> = emptySet()

    // ---- resolution ----

    /** Parameter-name lists of every method the call could reach with [argCount] arguments. */
    private fun callees(call: PsiElement, argCount: Int): List<List<String>> {
        val callee = call.firstChild ?: return emptyList()
        val methods: List<PsiElement> = when (callee.elementType) {
            E.FIELD_ACCESS_EXPRESSION -> {
                val name = JuxTypeEngine.memberName(callee) ?: return emptyList()
                val qualifier = JuxTypeEngine.firstExpressionChild(callee) ?: return emptyList()
                JuxTypeEngine.membersOfAllOverloads(JuxTypeEngine.typeOf(qualifier), name)
            }
            E.REFERENCE_EXPRESSION -> {
                val target = JuxTypeEngine.resolveReferenceExpression(callee, argCount) ?: return emptyList()
                if (target.elementType !== E.METHOD_DECLARATION) return emptyList()
                siblingsNamed(target)
            }
            else -> emptyList()
        }
        return methods
            .filter { it.elementType === E.METHOD_DECLARATION && JuxHierarchy.arity(it) == argCount }
            .map { paramNames(it) }
    }

    /**
     * Every overload that shares [target]'s name where [target] lives: its
     * class and supertypes for a member, the file for a free function.
     */
    private fun siblingsNamed(target: PsiElement): List<PsiElement> {
        val name = (target as? JuxNamedElement)?.name ?: return listOf(target)
        val owner = PsiTreeUtil.getParentOfType(target, JuxTypeDeclaration::class.java)
        if (owner != null) return JuxTypeEngine.membersOfAllOverloads(JuxTypeEngine.selfType(owner), name)
        val file = target.containingFile as? JuxFile ?: return listOf(target)
        return file.children.filter { (it as? JuxNamedElement)?.name == name }
    }

    /** Parameter-name lists of the constructors of `new T(…)` taking [argCount] arguments. */
    private fun constructors(newExpr: PsiElement, argCount: Int): List<List<String>> {
        val typeRef = newExpr.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return emptyList()
        val type = JuxTypeIndex.findType(newExpr, JuxHierarchy.bareTypeName(typeRef)) ?: return emptyList()
        val ctors = JuxHierarchy.directChildren(type, E.CONSTRUCTOR_DECLARATION)
        if (ctors.isNotEmpty()) {
            return ctors.filter { JuxHierarchy.arity(it) == argCount }.map { paramNames(it) }
        }
        // A record without a written constructor is built from its components.
        val components = type.node.findChildByType(E.RECORD_COMPONENT_LIST)?.psi?.children
            ?.filter { it.elementType === E.RECORD_COMPONENT }
            ?.map { (it as? JuxNamedElement)?.name ?: lastIdentifier(it) ?: "" }
            ?: return emptyList()
        return if (components.size == argCount) listOf(components) else emptyList()
    }

    /** The parameter names of a method or constructor, in order. */
    private fun paramNames(method: PsiElement): List<String> =
        JuxHierarchy.parameters(method).map { (it as? JuxNamedElement)?.name ?: lastIdentifier(it) ?: "" }

    /**
     * True when the argument already says the parameter's name: a bare
     * `name`, or a member read ending in `.name` / a getter-like `name()`.
     */
    private fun sameAsArgument(arg: PsiElement, name: String): Boolean {
        val last = when (arg.elementType) {
            E.REFERENCE_EXPRESSION, E.FIELD_ACCESS_EXPRESSION -> JuxTypeEngine.memberName(arg)
            E.CALL_EXPRESSION -> arg.firstChild?.let { JuxTypeEngine.memberName(it) }
            else -> null
        } ?: return false
        return last.equals(name, ignoreCase = true)
    }

    /** The last IDENTIFIER token within `el`'s subtree (or `el` itself). */
    private fun lastIdentifier(el: PsiElement): String? {
        if (el.elementType === JuxTokenTypes.IDENTIFIER) return el.text
        return PsiTreeUtil.collectElements(el) { it.elementType === JuxTokenTypes.IDENTIFIER }
            .lastOrNull()?.text
    }
}
