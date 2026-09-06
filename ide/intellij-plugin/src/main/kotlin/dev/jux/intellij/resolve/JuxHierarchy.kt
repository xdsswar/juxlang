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
            // DIRECT children only — a supertype is a top-level TYPE_REFERENCE
            // in the clause. A recursive walk would also pick up the type
            // ARGUMENTS nested inside a generic supertype (`implements
            // Holder<Object>` → the `Object` arg), wrongly flagging them as
            // separately-implemented types (false E0424).
            for (ref in clause.children) {
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
     * The depth-1 type arguments of a generic supertype reference, in order —
     * `Holder<Object>` → `["Object"]`, `Map<K, V>` → `["K", "V"]`. Each is the
     * argument's source text (trimmed); wildcards come through verbatim.
     * Empty for a non-generic reference.
     */
    fun typeArguments(ref: PsiElement): List<String> {
        val args = ref.node.findChildByType(JuxElementTypes.TYPE_ARGUMENT_LIST)?.psi ?: return emptyList()
        return args.children
            .filter {
                it.node.elementType === JuxElementTypes.TYPE_REFERENCE ||
                    it.node.elementType === JuxElementTypes.WILDCARD_TYPE
            }
            .map { it.text.trim() }
    }

    /**
     * The declared type-parameter names of a type — `class Box<T>` → `["T"]`,
     * `Map<K, V>` → `["K", "V"]`. Empty for a non-generic type.
     */
    fun typeParameterNames(type: JuxTypeDeclaration): List<String> {
        val list = type.node.findChildByType(JuxElementTypes.TYPE_PARAMETER_LIST)?.psi ?: return emptyList()
        return list.children
            .filter { it.node.elementType === JuxElementTypes.TYPE_PARAMETER }
            .mapNotNull { p ->
                var c: PsiElement? = p.firstChild
                while (c != null) {
                    if (c.node.elementType === dev.jux.intellij.highlight.JuxTokenTypes.IDENTIFIER) return@mapNotNull c.text
                    c = c.nextSibling
                }
                null
            }
    }

    /**
     * Type-parameter name → bound concrete-argument text, from `type`'s DIRECT
     * extends/implements clauses. `class C implements Holder<Animal>` (Holder
     * is `Holder<T>`) yields `{T -> "Animal"}`. Only positions with a matching
     * argument bind; the class's OWN params (which shadow) are excluded; a name
     * a clause binds two different ways is dropped (ambiguous). This is the map
     * the override generator substitutes with, and the
     * [dev.jux.intellij.inspections.JuxInheritedTypeParamInspection] uses to say
     * "use `Animal`" when the user writes the bare `T`.
     */
    fun inheritedTypeParameterBindings(type: JuxTypeDeclaration): Map<String, String> {
        val own = typeParameterNames(type).toHashSet()
        val out = HashMap<String, String>()
        val conflict = HashSet<String>()
        for ((ref, _) in supertypeReferences(type)) {
            val args = typeArguments(ref)
            if (args.isEmpty()) continue
            val superDecl = JuxTypeIndex.findType(ref, bareTypeName(ref)) ?: continue
            val params = typeParameterNames(superDecl)
            val bound = minOf(params.size, args.size)
            for (i in 0 until bound) {
                val name = params[i]
                if (name in own) continue
                val arg = args[i]
                val existing = out[name]
                if (existing != null && existing != arg) conflict.add(name) else out[name] = arg
            }
        }
        for (c in conflict) out.remove(c)
        return out
    }

    /**
     * Substitute whole-word type-parameter names in a signature/type string
     * with their bound arguments — `void test(T t)` + `{T=Object}` →
     * `void test(Object t)`. Single pass over identifier tokens (no
     * double-substitution); formal parameter NAMES and unrelated identifiers
     * pass through unchanged (case-sensitive).
     */
    fun substituteTypeParams(text: String, subst: Map<String, String>): String {
        if (subst.isEmpty()) return text
        return IDENT.replace(text) { m -> subst[m.value] ?: m.value }
    }

    private val IDENT = Regex("""[A-Za-z_]\w*""")

    /**
     * True when [name] is a type parameter DECLARED by an enclosing method or
     * type (`class Box<T>` / `<R> R f()`) — i.e. genuinely in scope, not merely
     * inherited. Shared by the annotator and the inherited-param inspection.
     */
    fun isDeclaredTypeParameter(at: PsiElement, name: String): Boolean {
        var scope: PsiElement? = at.parent
        while (scope != null) {
            when (scope.elementType) {
                JuxElementTypes.CLASS_DECLARATION, JuxElementTypes.INTERFACE_DECLARATION,
                JuxElementTypes.ENUM_DECLARATION, JuxElementTypes.RECORD_DECLARATION,
                JuxElementTypes.STRUCT_DECLARATION, JuxElementTypes.METHOD_DECLARATION,
                JuxElementTypes.TYPE_ALIAS_DECLARATION,
                -> {
                    val params = scope.node.findChildByType(JuxElementTypes.TYPE_PARAMETER_LIST)?.psi
                    if (params != null) {
                        var p: PsiElement? = params.firstChild
                        while (p != null) {
                            if (p.elementType === JuxElementTypes.TYPE_PARAMETER) {
                                var c: PsiElement? = p.firstChild
                                while (c != null) {
                                    if (c.node.elementType === dev.jux.intellij.highlight.JuxTokenTypes.IDENTIFIER) {
                                        if (c.text == name) return true
                                        break
                                    }
                                    c = c.nextSibling
                                }
                            }
                            p = p.nextSibling
                        }
                    }
                }
            }
            scope = scope.parent
        }
        return false
    }

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
    fun kindWord(type: JuxTypeDeclaration): String {
        val noun = kindNoun(type)
        return if (noun.first() in "aeiou") "an $noun" else "a $noun"
    }

    /**
     * The declaration's kind with no article — "interface", "record", "class".
     *
     * What a label wants: a breadcrumb tooltip or a hierarchy node reads
     * "interface Drawable", not "an interface Drawable". [kindWord] adds the
     * article for the prose that needs one.
     */
    fun kindNoun(type: JuxTypeDeclaration): String = when (type.node.elementType) {
        JuxElementTypes.INTERFACE_DECLARATION -> "interface"
        JuxElementTypes.RECORD_DECLARATION -> "record"
        JuxElementTypes.ENUM_DECLARATION -> "enum"
        JuxElementTypes.TYPE_ALIAS_DECLARATION -> "type alias"
        JuxElementTypes.STRUCT_DECLARATION -> "struct"
        JuxElementTypes.ANNOTATION_DECLARATION -> "annotation"
        else -> "class"
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
    fun parameterNames(m: PsiElement): List<String> =
        parameters(m).mapNotNull { (it as? JuxNamedElement)?.name }

    /**
     * The method's PARAMETER nodes, in declaration order.
     *
     * A parameter node spans everything the author wrote for it — leading
     * annotations and modifiers, the type, a `...` varargs marker, the name,
     * and any default (`parseParameter` in `JuxParser`). Rendering one is
     * therefore just its text with whitespace normalized, which is what the
     * parameter-info popup shows.
     */
    fun parameters(m: PsiElement): List<PsiElement> {
        val list = m.node.findChildByType(JuxElementTypes.PARAMETER_LIST)?.psi ?: return emptyList()
        return list.children.filter { it.node.elementType === JuxElementTypes.PARAMETER }
    }

    /** Direct children of `type`'s body with the given element type. */
    fun directChildren(type: JuxTypeDeclaration, et: IElementType): List<PsiElement> {
        val body = type.node.findChildByType(JuxElementTypes.CLASS_BODY)?.psi ?: return emptyList()
        return body.children.filter { it.node.elementType == et }
    }

    /**
     * A record's component names — `record Pt(int x, int y)` → `["x", "y"]`.
     * Empty for every other kind of declaration.
     *
     * Components ARE the record's accessors: `record Pt(int x, int y)
     * implements Point` satisfies `interface Point { int x(); int y(); }`
     * without declaring a method, which is the point of the form. Anything
     * asking "does this type provide a no-argument member named x?" has to
     * count them, or it reports a record that compiles as unimplemented.
     */
    fun recordComponentNames(type: JuxTypeDeclaration): List<String> =
        recordComponents(type).mapNotNull { (it as? JuxNamedElement)?.name }

    /** The RECORD_COMPONENT nodes of a record header, in declaration order. */
    fun recordComponents(type: JuxTypeDeclaration): List<PsiElement> {
        val list = type.node.findChildByType(JuxElementTypes.RECORD_COMPONENT_LIST)?.psi
            ?: return emptyList()
        return list.children.filter { it.node.elementType === JuxElementTypes.RECORD_COMPONENT }
    }

    /**
     * Whether [member] is reachable from inside [from] — Java's rules.
     *
     * `public` always; `private` only within the declaring type; `protected`
     * there or in a subtype. Package-level and `internal` are treated as
     * visible, the same call the language server makes
     * (`intel::member_visible`): the IDE cannot always know the package
     * relationship, and offering a name that turns out to be unreachable is a
     * far smaller failure than hiding one that is.
     *
     * `from` is null at the top level, where only `public` is reachable.
     */
    fun memberVisibleFrom(member: PsiElement, from: JuxTypeDeclaration?): Boolean {
        val owner = PsiTreeUtil.getParentOfType(member, JuxTypeDeclaration::class.java)
            ?: return true
        if (from != null && owner === from) return true
        if (hasModifier(member, "private")) return false
        if (hasModifier(member, "protected")) {
            return from != null && inheritsFrom(from, owner.name)
        }
        return true
    }

    /** True when [type] is, or transitively extends/implements, a type named [ancestor]. */
    fun inheritsFrom(type: JuxTypeDeclaration, ancestor: String?): Boolean {
        if (ancestor == null) return false
        if (type.name == ancestor) return true
        val queue = ArrayDeque(superTypeNames(type))
        val seen = HashSet<String>()
        while (queue.isNotEmpty()) {
            val name = queue.removeFirst()
            if (!seen.add(name)) continue
            if (name == ancestor) return true
            val decl = JuxTypeIndex.findType(type, name) ?: continue
            queue.addAll(superTypeNames(decl))
        }
        return false
    }

    /**
     * Whether a TYPE declared in [type]'s file is nameable from a file in
     * package [fromPackage].
     *
     * §4.4's hierarchy: `public` anywhere; no modifier means "visible within
     * this package only"; `private` at top level means file-scope. `internal`
     * is module-scoped, and the IDE has no reliable module identity for a Jux
     * project, so it is treated as visible — hiding a name that is legal is the
     * worse of the two errors, and the compiler does not enforce type-level
     * visibility at all yet.
     */
    fun typeVisibleFrom(type: JuxTypeDeclaration, fromPackage: String): Boolean {
        if (hasModifier(type, "public") || hasModifier(type, "internal")) return true
        val declaring = dev.jux.intellij.completion.JuxAutoImport.packageOf(type)
        // A type with no package is at the root, reachable from anywhere; a
        // file with no package of its own is likewise not fenced out.
        if (declaring.isEmpty() || fromPackage.isEmpty()) return true
        return declaring == fromPackage
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

    /**
     * True when the method node carries a body.
     *
     * A `{ … }` block is the only method-body form Jux has: the compiler
     * rejects `int f() = expr;` outright, and the expression-bodied form
     * (`String Name -> "n";`) belongs to PROPERTIES, which are a different node
     * and are not asked this question. (The parser is lenient about `= expr;`
     * so a half-typed member recovers, but that is recovery, not a body.)
     */
    fun hasBody(m: PsiElement): Boolean =
        m.node.findChildByType(JuxElementTypes.CODE_BLOCK) != null

    /**
     * Walks the supertype chain of [type] (breadth-first, cycle-guarded) and
     * returns the first super-method matching [name]/[arity], or `null`.
     * The walk resolves type names project-wide through [JuxTypeIndex].
     */
    fun findSuperMethod(type: JuxTypeDeclaration, name: String, arity: Int): PsiElement? {
        // Each hop resolves FROM the declaration that named the supertype, so a
        // chain that stays inside one file never leaves it for a same-named
        // type elsewhere in the project.
        val queue = ArrayDeque(superTypeNames(type).map { it to type })
        val visited = HashSet<String>()
        while (queue.isNotEmpty()) {
            val (superName, owner) = queue.removeFirst()
            if (!visited.add(superName)) continue
            val superDecl = JuxTypeIndex.findType(owner, superName) ?: continue
            for (m in directChildren(superDecl, JuxElementTypes.METHOD_DECLARATION)) {
                val mName = (m as? JuxNamedElement)?.name ?: continue
                if (mName == name && arity(m) == arity && isOverridable(m)) return m
            }
            queue.addAll(superTypeNames(superDecl).map { it to superDecl })
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
            // A record's components are its fields AND its accessors, so they
            // belong in the member list `p.<caret>` completes from — the body
            // walk above cannot see them, since they live in the header.
            for (c in recordComponents(t)) {
                val name = (c as? JuxNamedElement)?.name ?: continue
                if (seen.add(name)) out.add(c)
            }
            for (sn in superTypeNames(t)) {
                JuxTypeIndex.findType(t, sn)?.let { queue.add(it) }
            }
        }
        return out
    }

    /** Member element types enumerated by [allMembers], in offer order. */
    private val MEMBER_KINDS = listOf(
        JuxElementTypes.METHOD_DECLARATION,
        JuxElementTypes.PROPERTY_DECLARATION,
        JuxElementTypes.FIELD_DECLARATION,
        // A `const` member is reached through its type exactly as a static
        // field is (`Limits.MAX`); leaving it out meant it completed as a bare
        // name inside the class but vanished after a dot.
        JuxElementTypes.CONST_DECLARATION,
        JuxElementTypes.ENUM_CONSTANT,
    )
}
