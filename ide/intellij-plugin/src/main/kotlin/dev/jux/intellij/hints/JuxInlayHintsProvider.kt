package dev.jux.intellij.hints

import com.intellij.codeInsight.hints.HintInfo
import com.intellij.codeInsight.hints.InlayInfo
import com.intellij.codeInsight.hints.InlayParameterHintsProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxMethodDeclaration

/**
 * Java-style **parameter name hints**: inside a resolved `foo(a, b)` /
 * `obj.foo(a, b)` call, show `paramName:` in front of each argument. Resolution
 * is by-name over the file's method/function declarations (the native PSI), so
 * it works without the LSP; cross-file / std calls simply get no hints.
 *
 * Overloads are disambiguated by argument count — a hint set is only produced
 * when exactly one declaration of that name has a matching arity, so the IDE
 * never shows a wrong label.
 */
class JuxInlayHintsProvider : InlayParameterHintsProvider {
    override fun getParameterHints(element: PsiElement): List<InlayInfo> {
        if (element.elementType !== E.CALL_EXPRESSION) return emptyList()
        val argList = generateSequence(element.firstChild) { it.nextSibling }
            .firstOrNull { it.elementType === E.ARGUMENT_LIST } ?: return emptyList()
        val argOffsets = argStartOffsets(argList)
        if (argOffsets.isEmpty()) return emptyList()

        val name = calleeName(element) ?: return emptyList()
        val params = resolveParamNames(element.containingFile, name, argOffsets.size) ?: return emptyList()

        val out = ArrayList<InlayInfo>(argOffsets.size)
        for (i in argOffsets.indices) {
            if (i >= params.size) break
            out.add(InlayInfo(params[i], argOffsets[i]))
        }
        return out
    }

    override fun getHintInfo(element: PsiElement): HintInfo? = null

    override fun getDefaultBlackList(): Set<String> = emptySet()

    // ---- call-site shape ----

    /**
     * The start offset of each argument: the first non-blank token following the
     * opening `(` or a `,`. Robust whether an argument is a single leaf or a
     * composite expression node.
     */
    private fun argStartOffsets(argList: PsiElement): List<Int> {
        val offsets = ArrayList<Int>()
        var expectArg = false
        var child: PsiElement? = argList.firstChild
        while (child != null) {
            val t = child.elementType
            when {
                t === JuxTokenTypes.LPAREN || t === JuxTokenTypes.COMMA -> expectArg = true
                t === JuxTokenTypes.RPAREN -> {}
                child.text.isBlank() -> {}
                expectArg -> {
                    offsets.add(child.textRange.startOffset)
                    expectArg = false
                }
            }
            child = child.nextSibling
        }
        return offsets
    }

    /** The called member's name — the last identifier in the callee subtree
     *  (`foo` → `foo`, `a.b.foo` → `foo`). */
    private fun calleeName(call: PsiElement): String? {
        val callee = generateSequence(call.firstChild) { it.nextSibling }
            .firstOrNull { it.elementType !== E.ARGUMENT_LIST && !it.text.isBlank() } ?: return null
        return lastIdentifier(callee)
    }

    // ---- declaration resolution ----

    /**
     * Parameter names of the unique file-local method/function named `name` that
     * takes `argCount` parameters. `null` when none — or more than one — matches
     * (ambiguous → no hints, so a label is never wrong).
     */
    private fun resolveParamNames(file: PsiElement?, name: String, argCount: Int): List<String>? {
        if (file == null) return null
        val matches = PsiTreeUtil.findChildrenOfType(file, JuxMethodDeclaration::class.java)
            .filter { it.name == name }
            .map { paramNames(it) }
            .filter { it.size == argCount }
        return matches.singleOrNull()
    }

    /** The parameter names of a method declaration, in order. */
    private fun paramNames(method: JuxMethodDeclaration): List<String> {
        val list = generateSequence(method.firstChild) { it.nextSibling }
            .firstOrNull { it.elementType === E.PARAMETER_LIST } ?: return emptyList()
        return generateSequence(list.firstChild) { it.nextSibling }
            .filter { it.elementType === E.PARAMETER }
            .mapNotNull { lastIdentifier(it) }
            .toList()
    }

    /** The last IDENTIFIER token within `el`'s subtree (or `el` itself). */
    private fun lastIdentifier(el: PsiElement): String? {
        if (el.elementType === JuxTokenTypes.IDENTIFIER) return el.text
        return PsiTreeUtil.collectElements(el) { it.elementType === JuxTokenTypes.IDENTIFIER }
            .lastOrNull()?.text
    }
}
