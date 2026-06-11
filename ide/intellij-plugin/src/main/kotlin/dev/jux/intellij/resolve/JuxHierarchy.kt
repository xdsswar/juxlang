package dev.jux.intellij.resolve

import com.intellij.psi.PsiElement
import com.intellij.psi.tree.IElementType
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * Shared supertype/signature walking over the Jux PSI — the single home for
 * "what does this class inherit?" questions. Used by the Alt+Insert
 * Override/Implement generator ([dev.jux.intellij.actions.JuxOverrideMethodsAction]),
 * the override/implement gutter markers ([JuxLineMarkerProvider]), and the
 * missing-`@override` inspection.
 *
 * Resolution is name-based via [JuxTypeIndex] (project-wide), so it works
 * without the LSP. Methods match by **name + arity** — Jux overloads exist,
 * but parameter *types* can't be compared reliably without the type checker,
 * and name+arity is the same approximation the generator has always used.
 */
object JuxHierarchy {
    /** Type names in `type`'s `extends` and `implements` clauses (bare last segment). */
    fun superTypeNames(type: JuxTypeDeclaration): List<String> {
        val out = ArrayList<String>()
        for (clauseType in listOf(JuxElementTypes.EXTENDS_CLAUSE, JuxElementTypes.IMPLEMENTS_CLAUSE)) {
            val clause = type.node.findChildByType(clauseType)?.psi ?: continue
            for (ref in PsiTreeUtil.findChildrenOfType(clause, PsiElement::class.java)) {
                if (ref.node.elementType == JuxElementTypes.TYPE_REFERENCE) {
                    ref.text.trim().substringAfterLast('.').substringBefore('<').trim()
                        .takeIf { it.isNotEmpty() }?.let(out::add)
                }
            }
        }
        return out
    }

    /** Direct children of `type`'s body with the given element type. */
    fun directChildren(type: JuxTypeDeclaration, et: IElementType): List<PsiElement> {
        val body = type.node.findChildByType(JuxElementTypes.CLASS_BODY)?.psi ?: return emptyList()
        return body.children.filter { it.node.elementType == et }
    }

    /** `static` / `private` / `final` methods can't be overridden. */
    fun isOverridable(m: PsiElement): Boolean {
        val mods = m.node.findChildByType(JuxElementTypes.MODIFIER_LIST)?.psi ?: return true
        val text = " ${mods.text} "
        return !text.contains(" static ") && !text.contains(" private ") && !text.contains(" final ")
    }

    /** The method's signature text: return type + name + param list (+ throws), no modifiers/body. */
    fun methodSignature(m: PsiElement): String? {
        val sb = StringBuilder()
        var c: PsiElement? = m.firstChild
        var sawParams = false
        while (c != null) {
            val t = c.node.elementType
            if (t == JuxElementTypes.MODIFIER_LIST) { c = c.nextSibling; continue }
            if (t == JuxElementTypes.CLASS_BODY || c.text == ";" || c.text == "{") break
            sb.append(c.text)
            if (t == JuxElementTypes.PARAMETER_LIST) sawParams = true
            c = c.nextSibling
        }
        return if (sawParams) sb.toString().trim().replace(Regex("\\s+"), " ") else null
    }

    /** Number of declared parameters of a method/constructor node. */
    fun arity(m: PsiElement): Int {
        val list = m.node.findChildByType(JuxElementTypes.PARAMETER_LIST)?.psi ?: return 0
        return list.children.count { it.elementType === JuxElementTypes.PARAMETER }
    }

    /** True when the method node carries a body (`{…}` block or `= expr` form). */
    fun hasBody(m: PsiElement): Boolean =
        m.node.findChildByType(JuxElementTypes.CODE_BLOCK) != null

    /**
     * Walks the supertype chain of [type] (breadth-first, cycle-guarded) and
     * returns the first super-method matching [name]/[arity], or `null`.
     * The walk resolves type names project-wide through [JuxTypeIndex].
     */
    fun findSuperMethod(type: JuxTypeDeclaration, name: String, arity: Int): PsiElement? {
        val project = type.project
        val queue = ArrayDeque(superTypeNames(type))
        val visited = HashSet<String>()
        while (queue.isNotEmpty()) {
            val superName = queue.removeFirst()
            if (!visited.add(superName)) continue
            val superDecl = JuxTypeIndex.findType(project, superName) ?: continue
            for (m in directChildren(superDecl, JuxElementTypes.METHOD_DECLARATION)) {
                val mName = (m as? JuxNamedElement)?.name ?: continue
                if (mName == name && arity(m) == arity && isOverridable(m)) return m
            }
            queue.addAll(superTypeNames(superDecl))
        }
        return null
    }

    /** The enclosing type declaration of a PSI element, or `null` at top level. */
    fun enclosingType(element: PsiElement): JuxTypeDeclaration? =
        PsiTreeUtil.getParentOfType(element, JuxTypeDeclaration::class.java)
}
