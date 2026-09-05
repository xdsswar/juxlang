package dev.jux.intellij.resolve

import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * Lightweight, in-file type inference for **member completion** (`recv.<caret>`)
 * — the IDE-side approximation of what `juxc-lsp` does with the full type
 * checker. It resolves a receiver to the type whose members should be offered,
 * for the common, statically-obvious shapes:
 *
 *  - `this` / `super` → the enclosing type (or its `extends` parent);
 *  - a local / parameter / field / property whose declared type is written
 *    (`Point p`, `Point field;`) or inferable from a `new T(...)` initializer
 *    (`var p = new Point();`);
 *  - a bare **type name** (`Color.`, `Math.`) → that type, in *static* mode.
 *
 * It deliberately does NOT attempt full expression typing (chained calls,
 * generics substitution, stdlib/Rust types) — those stay with the LSP. Every
 * lookup is project-wide via [JuxTypeIndex], so cross-file user types resolve.
 */
object JuxTypeInference {

    /** The type a receiver denotes, plus whether the access is static. */
    data class Target(val type: JuxTypeDeclaration, val isStatic: Boolean)

    /**
     * Resolve the receiver named [receiverWord] (the identifier immediately
     * before the `.`) as seen from [context] (the PSI element at the caret).
     * Returns null when the type can't be determined in-file — the caller then
     * offers nothing (member completion is the LSP's job for those).
     */
    fun resolveReceiver(receiverWord: String, context: PsiElement): Target? {
        // `this` / `super` — the enclosing type, or its extends parent.
        if (receiverWord == "this" || receiverWord == "super") {
            val enclosing = PsiTreeUtil.getParentOfType(context, JuxTypeDeclaration::class.java) ?: return null
            if (receiverWord == "super") {
                val parentName = JuxHierarchy.superTypeNames(enclosing).firstOrNull() ?: return null
                val parent = JuxTypeIndex.findType(context, parentName) ?: return null
                return Target(parent, isStatic = false)
            }
            return Target(enclosing, isStatic = false)
        }

        // A value declaration (local / param / field / property) visible here?
        val decl = resolveValueDecl(receiverWord, context)
        if (decl != null) {
            val typeName = declaredTypeName(decl) ?: return null
            val type = JuxTypeIndex.findType(context, typeName) ?: return null
            return Target(type, isStatic = false)
        }

        // Otherwise the word may name a TYPE → static-member access.
        val type = JuxTypeIndex.findType(context, receiverWord) ?: return null
        return Target(type, isStatic = true)
    }

    /**
     * Find the value declaration named [name] visible from [context], walking
     * enclosing scopes innermost-out — same shape as
     * [JuxReference.resolveLocally] but restricted to value (non-type)
     * declarations: locals, parameters, fields, properties.
     */
    private fun resolveValueDecl(name: String, context: PsiElement): JuxNamedElement? {
        val offset = context.textOffset
        var scope: PsiElement? = context.parent
        while (scope != null) {
            when (scope.elementType) {
                E.CODE_BLOCK ->
                    for (child in scope.children) {
                        if (child.elementType === E.LOCAL_VARIABLE && child.textOffset < offset &&
                            (child as? JuxNamedElement)?.name == name
                        ) return child as JuxNamedElement
                    }
                // Loop, catch and lambda bindings — see JuxReference for the
            // matching resolution walk; a receiver named by one of them has to
            // type-resolve too.
            E.FOR_EACH_STATEMENT, E.FOR_STATEMENT, E.CATCH_CLAUSE ->
                scope.children.firstOrNull {
                    it.elementType === E.LOCAL_VARIABLE && (it as? JuxNamedElement)?.name == name
                }?.let { return it as JuxNamedElement }
            E.LAMBDA_EXPRESSION -> {
                val list = scope.children.firstOrNull { it.elementType === E.PARAMETER_LIST }
                val params = (list?.children?.toList() ?: emptyList()) + scope.children
                params.firstOrNull {
                    it.elementType === E.PARAMETER && (it as? JuxNamedElement)?.name == name
                }?.let { return it as JuxNamedElement }
            }
            E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                    scope.children.firstOrNull { it.elementType === E.PARAMETER_LIST }
                        ?.children?.forEach { p ->
                            if (p.elementType === E.PARAMETER && (p as? JuxNamedElement)?.name == name) {
                                return p as JuxNamedElement
                            }
                        }
                E.CLASS_BODY ->
                    for (m in scope.children) {
                        if ((m.elementType === E.FIELD_DECLARATION || m.elementType === E.PROPERTY_DECLARATION) &&
                            (m as? JuxNamedElement)?.name == name
                        ) return m as JuxNamedElement
                    }
            }
            if (scope is JuxFile) break
            scope = scope.parent
        }
        return null
    }

    /**
     * A written type with its arguments — `Vec<Leaf>` → `("Vec", ["Leaf"])`.
     *
     * The resolver has to carry the arguments, not just the resolved
     * declaration: the element type of `xs[0]` lives in the ARGUMENT the use
     * site wrote, and by the time a name has been resolved to a
     * [JuxTypeDeclaration] that information is gone.
     */
    private data class TypeInfo(val bare: String, val args: List<String>)

    /**
     * Resolve a whole receiver EXPRESSION — `n.make()!!.leaf`, `xs[0]`,
     * `(n.leaf)` — to the type whose members belong after the following `.`.
     *
     * The old resolver read one identifier backwards from the dot, so anything
     * ending in `)`, `]`, `!` or `?` produced no receiver at all. Since the
     * after-dot branch returns unconditionally, that showed an EMPTY popup
     * rather than falling back to anything — on shapes real code is full of.
     *
     * The expression is split into steps left to right and each is applied to
     * the type so far:
     *
     * - a leading word → a local, parameter, field or property's declared type,
     *   `this`/`super`, or a type name for static access;
     * - `.name` → that member's declared type; for a method, its return type;
     * - `(…)` → belongs to the method named just before it, which already
     *   produced that method's return type;
     * - `!!` and `?` → the non-null view, which for a name-based resolver is
     *   the same type;
     * - `[…]` → the element type: the receiver's first type argument.
     *
     * Anything it cannot follow yields null, and the caller shows nothing
     * rather than guessing. Full expression typing — arithmetic, ternaries,
     * generic substitution down a chain — stays with the language server; this
     * is the shape of receiver the fallback must simply not choke on.
     */
    fun resolveReceiverExpression(expression: String, context: PsiElement): Target? {
        val steps = splitSteps(expression) ?: return null
        val first = steps.firstOrNull() ?: return null

        // `this` / `super` / a bare type name keep the existing base handling —
        // they have no written type arguments to carry.
        if (steps.size == 1) return resolveReceiver(first, context)

        // A value's written type, or — for `this`/`super`/a bare type name —
        // the resolved declaration's own name.
        var info: TypeInfo = baseTypeInfo(first, context)
            ?: run {
                val base = resolveReceiver(first, context) ?: return null
                TypeInfo(base.type.name ?: return null, emptyList())
            }

        for (step in steps.drop(1)) {
            info = when {
                step == "!!" || step == "?" -> info
                step.startsWith("(") -> info
                step.startsWith("[") -> TypeInfo(
                    bareOf(info.args.firstOrNull() ?: return null),
                    argsOf(info.args.firstOrNull() ?: return null),
                )
                else -> memberTypeInfo(info, step, context) ?: return null
            }
        }
        val decl = JuxTypeIndex.findType(context, info.bare) ?: return null
        return Target(decl, isStatic = false)
    }

    /** The written type of a value named [word] visible from [context]. */
    private fun baseTypeInfo(word: String, context: PsiElement): TypeInfo? {
        if (word == "this" || word == "super") {
            val target = resolveReceiver(word, context) ?: return null
            return TypeInfo(target.type.name ?: return null, emptyList())
        }
        val decl = resolveValueDecl(word, context) ?: return null
        val typeRef = (decl as PsiElement).node.findChildByType(E.TYPE_REFERENCE)?.psi
            ?: return declaredTypeName(decl)?.let { TypeInfo(it, emptyList()) }
        return TypeInfo(bareName(typeRef), JuxHierarchy.typeArguments(typeRef))
    }

    /** The written type of [name] as a member of [info] — a field/property type or a return type. */
    private fun memberTypeInfo(info: TypeInfo, name: String, context: PsiElement): TypeInfo? {
        val owner = JuxTypeIndex.findType(context, info.bare) ?: return null
        val member = JuxHierarchy.allMembers(owner)
            .firstOrNull { (it as? JuxNamedElement)?.name == name } ?: return null
        // A method's first TYPE_REFERENCE is its return type; a field's or a
        // property's is its declared type — the same child either way.
        val typeRef = member.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return null
        return TypeInfo(bareName(typeRef), JuxHierarchy.typeArguments(typeRef))
    }

    /**
     * Split a receiver expression into ordered steps: a leading name, then any
     * of `.name`, `(…)`, `[…]`, `!!`, `?`.
     *
     * Null when the expression holds something this resolver does not model —
     * an operator, a literal, a cast. Offering nothing beats resolving the
     * wrong half of `a + b`.
     */
    private fun splitSteps(expression: String): List<String>? {
        val e = expression.trim()
        if (e.isEmpty()) return null
        // A wholly parenthesized expression is transparent: `(n.leaf)` → `n.leaf`.
        if (e.startsWith("(") && matchingClose(e, 0, '(', ')') == e.length - 1) {
            return splitSteps(e.substring(1, e.length - 1))
        }
        val steps = ArrayList<String>()
        val sb = StringBuilder()
        fun flush() {
            if (sb.isNotEmpty()) {
                steps.add(sb.toString())
                sb.clear()
            }
        }
        var i = 0
        while (i < e.length) {
            val c = e[i]
            when {
                c.isLetterOrDigit() || c == '_' -> { sb.append(c); i++ }
                c == '.' -> { flush(); i++ }
                c == '!' && i + 1 < e.length && e[i + 1] == '!' -> { flush(); steps.add("!!"); i += 2 }
                c == '?' -> { flush(); steps.add("?"); i++ }
                c == '(' || c == '[' -> {
                    flush()
                    val close = if (c == '(') ')' else ']'
                    val end = matchingClose(e, i, c, close) ?: return null
                    steps.add(e.substring(i, end + 1))
                    i = end + 1
                }
                c.isWhitespace() -> i++
                else -> return null
            }
        }
        flush()
        return steps.ifEmpty { null }
    }

    /** Index of the bracket closing the one at [open], or null when unbalanced. */
    private fun matchingClose(text: String, open: Int, openCh: Char, closeCh: Char): Int? {
        var depth = 0
        var i = open
        while (i < text.length) {
            if (text[i] == openCh) depth++
            if (text[i] == closeCh) {
                depth--
                if (depth == 0) return i
            }
            i++
        }
        return null
    }

    /**
     * `Vec<Leaf>` → `Vec`, `a.b.Leaf?` → `Leaf`; a bare name is unchanged.
     *
     * The trailing `?` goes with the generics: a nullable type's members are
     * the underlying type's, reached through `!!` or `?.`.
     */
    private fun bareOf(typeText: String): String =
        typeText.trim()
            .substringAfterLast('.')
            .substringBefore('<')
            .trim()
            .removeSuffix("?")
            .trim()

    /** `Map<K, Vec<V>>` → `["K", "Vec<V>"]`; depth-aware so nesting survives. */
    private fun argsOf(typeText: String): List<String> {
        val open = typeText.indexOf('<')
        if (open < 0 || !typeText.trim().endsWith('>')) return emptyList()
        val inner = typeText.trim().removeSuffix(">").substring(open + 1)
        val out = ArrayList<String>()
        var depth = 0
        val sb = StringBuilder()
        for (c in inner) {
            when {
                c == '<' -> { depth++; sb.append(c) }
                c == '>' -> { depth--; sb.append(c) }
                c == ',' && depth == 0 -> { out.add(sb.toString().trim()); sb.clear() }
                else -> sb.append(c)
            }
        }
        if (sb.isNotBlank()) out.add(sb.toString().trim())
        return out
    }

    /**
     * The bare type name a value declaration introduces: the written
     * `TYPE_REFERENCE` if present, else inferred from a `new T(...)`
     * initializer on a `var` local. Returns null when no type is recoverable.
     */
    private fun declaredTypeName(decl: JuxNamedElement): String? {
        val node = (decl as PsiElement)
        // Explicit type: `Point p`, `Point field;`, `Point Prop { get; set; }`.
        node.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let { return bareName(it) }
        // `var p = new Point();` — infer from the initializer's new-expression.
        if (node.elementType === E.LOCAL_VARIABLE) {
            val newExpr = PsiTreeUtil.findChildrenOfType(node, PsiElement::class.java)
                .firstOrNull { it.elementType === E.NEW_EXPRESSION } ?: return null
            newExpr.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let { return bareName(it) }
        }
        return null
    }

    /**
     * Last segment of a TYPE_REFERENCE, generics and the nullable marker
     * stripped (`a.b.List<int>` → `List`, `Leaf?` → `Leaf`).
     *
     * Dropping the `?` is what makes `x!!.` and `x?.` resolve: the step is the
     * NON-NULL view of the same type, and a name-based resolver has nothing
     * else to represent that with.
     */
    private fun bareName(typeRef: PsiElement): String = bareOf(typeRef.text)
}
