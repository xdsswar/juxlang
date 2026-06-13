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
    fun superTypeNames(type: JuxTypeDeclaration): List<String> =
        supertypeReferences(type).map { (ref, _) -> bareTypeName(ref) }.filter { it.isNotEmpty() }

    /**
     * The TYPE_REFERENCE nodes of `type`'s supertype clauses, in source order,
     * each paired with `true` when it sits in the `extends` clause (`false` =
     * `implements`). The PSI-node form [superTypeNames] throws away — needed by
     * the extends/implements clause inspections to highlight a specific entry.
     */
    fun supertypeReferences(type: JuxTypeDeclaration): List<Pair<PsiElement, Boolean>> {
        val out = ArrayList<Pair<PsiElement, Boolean>>()
        for ((clauseType, isExtends) in listOf(
            JuxElementTypes.EXTENDS_CLAUSE to true,
            JuxElementTypes.IMPLEMENTS_CLAUSE to false,
        )) {
            val clause = type.node.findChildByType(clauseType)?.psi ?: continue
            for (ref in PsiTreeUtil.findChildrenOfType(clause, PsiElement::class.java)) {
                if (ref.node.elementType == JuxElementTypes.TYPE_REFERENCE) {
                    out.add(ref to isExtends)
                }
            }
        }
        return out
    }

    /** The bare type name of a TYPE_REFERENCE: last segment, generics stripped. */
    fun bareTypeName(ref: PsiElement): String =
        ref.text.trim().substringAfterLast('.').substringBefore('<').trim()

    /**
     * Does the declaration carry modifier [kw]? Modifiers are always wrapped
     * in a MODIFIER_LIST composite (never direct keyword children), so the
     * check reads that list's text. Shared by the Generate actions, the
     * override engine, and the inheritance inspections.
     */
    fun hasModifier(el: PsiElement, kw: String): Boolean {
        val mods = el.node.findChildByType(JuxElementTypes.MODIFIER_LIST)?.text ?: return false
        return " $mods ".contains(" $kw ")
    }

    /** True for an `interface` declaration. */
    fun isInterface(type: JuxTypeDeclaration): Boolean =
        type.node.elementType === JuxElementTypes.INTERFACE_DECLARATION

    /** True for a `class` declaration (the only extensible kind, §6.1 / E0423). */
    fun isClass(type: JuxTypeDeclaration): Boolean =
        type.node.elementType === JuxElementTypes.CLASS_DECLARATION

    /**
     * The declaration's kind as the compiler's E0423/E0424 wording names it —
     * "an interface" / "a record" / "an enum" / "a type alias" / "a class".
     */
    fun kindWord(type: JuxTypeDeclaration): String = when (type.node.elementType) {
        JuxElementTypes.INTERFACE_DECLARATION -> "an interface"
        JuxElementTypes.RECORD_DECLARATION -> "a record"
        JuxElementTypes.ENUM_DECLARATION -> "an enum"
        JuxElementTypes.TYPE_ALIAS_DECLARATION -> "a type alias"
        JuxElementTypes.STRUCT_DECLARATION -> "a struct"
        JuxElementTypes.ANNOTATION_DECLARATION -> "an annotation"
        else -> "a class"
    }

    /**
     * True when the type never needs to implement inherited abstract methods
     * itself: interfaces always, classes declared `abstract`.
     */
    fun isAbstractType(type: JuxTypeDeclaration): Boolean =
        isInterface(type) || hasModifier(type, "abstract")

    /**
     * True for a body-less method — an interface method without a `default`
     * body, or an `abstract` class method. Same CODE_BLOCK rule the
     * override/implement gutter classifier uses.
     */
    fun isAbstractMethod(m: PsiElement): Boolean = !hasBody(m)

    /**
     * The method's declared return type text, or null when unreadable. The
     * return type is the first TYPE_REFERENCE direct child (it precedes the
     * name; parameter types are nested inside PARAMETER_LIST). `void` parses
     * as a TYPE_REFERENCE holding just the keyword.
     */
    fun returnTypeText(m: PsiElement): String? =
        m.node.findChildByType(JuxElementTypes.TYPE_REFERENCE)?.text?.trim()

    /** The method's parameter names, in declaration order. */
    fun parameterNames(m: PsiElement): List<String> {
        val list = m.node.findChildByType(JuxElementTypes.PARAMETER_LIST)?.psi ?: return emptyList()
        return list.children
            .filter { it.elementType === JuxElementTypes.PARAMETER }
            .mapNotNull { (it as? JuxNamedElement)?.name }
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

    /**
     * Every member declaration of [type] and its supertypes — methods, fields,
     * properties, and enum constants — nearest-declaration first, deduped so an
     * override / shadow appears once (key: name for fields/properties/enum
     * constants, name+arity for methods, so overloads stay distinct). Powers
     * member completion (`recv.<caret>`). Cross-file supertypes resolve via
     * [JuxTypeIndex]; the walk is breadth-first and cycle-guarded.
     */
    fun allMembers(type: JuxTypeDeclaration): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        val seen = HashSet<String>()
        val queue = ArrayDeque<JuxTypeDeclaration>()
        queue.add(type)
        val visitedTypes = HashSet<String>()
        while (queue.isNotEmpty()) {
            val t = queue.removeFirst()
            val tn = t.name ?: continue
            if (!visitedTypes.add(tn)) continue
            for (et in MEMBER_KINDS) {
                for (m in directChildren(t, et)) {
                    val name = (m as? JuxNamedElement)?.name ?: continue
                    val key = if (et === JuxElementTypes.METHOD_DECLARATION) "$name/${arity(m)}()" else name
                    if (seen.add(key)) out.add(m)
                }
            }
            for (sn in superTypeNames(t)) {
                JuxTypeIndex.findType(t.project, sn)?.let { queue.add(it) }
            }
        }
        return out
    }

    /** Member element types enumerated by [allMembers], in offer order. */
    private val MEMBER_KINDS = listOf(
        JuxElementTypes.METHOD_DECLARATION,
        JuxElementTypes.PROPERTY_DECLARATION,
        JuxElementTypes.FIELD_DECLARATION,
        JuxElementTypes.ENUM_CONSTANT,
    )
}
