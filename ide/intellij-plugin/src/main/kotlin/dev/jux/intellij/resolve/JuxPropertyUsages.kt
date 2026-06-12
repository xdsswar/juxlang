package dev.jux.intellij.resolve

import com.intellij.openapi.util.Key
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.CachedValue
import com.intellij.psi.util.CachedValueProvider
import com.intellij.psi.util.CachedValuesManager
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxObservableProps
import dev.jux.intellij.psi.JuxPropertyDeclaration
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * One cached pass per file over the §P observer/binding call surface — the
 * shared backbone of the W0971 (never observed), E0973 (assignment to bound
 * property), and E0974 (bind type mismatch) inspections and the property
 * gutter provider. Cached per file ([CachedValuesManager], invalidated on file
 * change) exactly like [JuxTypeIndex.typesIn], because the gutter provider and
 * three inspections would otherwise each re-walk the full PSI per daemon pass.
 *
 * Matching is by tree shape + token text (the annotator's rules): a call whose
 * callee is `<recv>.observers.attach` / `<recv>.bind` etc. Receivers are keyed
 * two ways — by their **final name** (`Name` of `m.Name`, for per-property
 * collection like W0971 and the gutter) and by their **whitespace-stripped
 * chain text** (`m.Name`, for the E0973 exact-receiver match that kills
 * cross-object false positives).
 */
object JuxPropertyUsages {

    /** Everything §P-shaped found in one file. */
    class FileUsages(
        /** Final property name → `X.observers.attach(…)` / `.detach(…)` call expressions. */
        val attachSites: Map<String, List<PsiElement>>,
        /** Final property name → `X.bind(…)` / `X.bindBidirectional(…)` call expressions. */
        val bindSites: Map<String, List<PsiElement>>,
        /**
         * Receiver chain text → one-way `bind` calls whose receiver it is —
         * the E0973 targets. Bidirectional bindings are deliberately excluded:
         * their internal `updating` guard makes direct sets legal (§P.4.3).
         */
        val bindTargets: Map<String, List<PsiElement>>,
        /** Final names of properties appearing as `bind(…)` ARGUMENTS (binding sources). */
        val bindSources: Set<String>,
        /** Receiver chain texts of `X.unbind()` calls — E0973 suppressors. */
        val unbindTargets: Set<String>,
    )

    private val KEY: Key<CachedValue<FileUsages>> = Key.create("jux.file.property.usages")

    /** The file's §P usage map, cached until the file changes. */
    fun usagesIn(file: PsiFile): FileUsages =
        CachedValuesManager.getManager(file.project).getCachedValue(file, KEY, {
            CachedValueProvider.Result.create(build(file), file)
        }, false)

    private fun build(file: PsiFile): FileUsages {
        val attachSites = HashMap<String, MutableList<PsiElement>>()
        val bindSites = HashMap<String, MutableList<PsiElement>>()
        val bindTargets = HashMap<String, MutableList<PsiElement>>()
        val bindSources = HashSet<String>()
        val unbindTargets = HashSet<String>()

        val calls = PsiTreeUtil.collectElements(file) { it.elementType === E.CALL_EXPRESSION }
        for (call in calls) {
            val callee = call.firstChild ?: continue
            if (callee.elementType !== E.FIELD_ACCESS_EXPRESSION) continue
            val op = lastIdentifier(callee)?.text ?: continue
            val recv = callee.firstChild ?: continue

            when (op) {
                // `<prop>.observers.attach(…)` / `.detach(…)`
                "attach", "detach" -> {
                    if (recv.elementType !== E.FIELD_ACCESS_EXPRESSION) continue
                    if (lastIdentifier(recv)?.text != JuxObservableProps.OBSERVERS_MEMBER) continue
                    val prop = recv.firstChild ?: continue
                    val name = lastIdentifier(prop)?.text ?: continue
                    attachSites.getOrPut(name) { ArrayList() }.add(call)
                }
                // `<prop>.bind(src)` / `<prop>.bindBidirectional(other)`
                "bind", "bindBidirectional" -> {
                    if (recv.elementType !in RECEIVERS) continue
                    val name = lastIdentifier(recv)?.text ?: continue
                    bindSites.getOrPut(name) { ArrayList() }.add(call)
                    if (op == "bind") {
                        bindTargets.getOrPut(chainText(recv)) { ArrayList() }.add(call)
                    }
                    // The argument property is a binding source (gutter: 🔗).
                    bindArgument(call)?.let { arg ->
                        lastIdentifier(arg)?.text?.let(bindSources::add)
                    }
                }
                // `<prop>.unbind()`
                "unbind" -> {
                    if (recv.elementType !in RECEIVERS) continue
                    unbindTargets.add(chainText(recv))
                }
            }
        }
        return FileUsages(attachSites, bindSites, bindTargets, bindSources, unbindTargets)
    }

    /** The first expression in the call's argument list, when reference-shaped. */
    fun bindArgument(call: PsiElement): PsiElement? {
        val args = call.children.firstOrNull { it.elementType === E.ARGUMENT_LIST } ?: return null
        return args.children.firstOrNull { it.elementType in RECEIVERS }
    }

    /**
     * Resolve a `bind`/`bindBidirectional` operand expression to the
     * [JuxPropertyDeclaration] it denotes, or null when it can't be pinned
     * down. Precision-first (E0974 is an IDE-only error today — a false
     * positive paints un-suppressable red on legal code):
     *
     * - `Name` / `this.Name` — in-file scope walk (members resolve through
     *   the CLASS_BODY scope);
     * - `q.Name` — `q` must itself resolve in-file to a declaration with a
     *   visible `TYPE_REFERENCE`; its type name goes through [JuxTypeIndex]
     *   and `Name` is looked up among that type's property members.
     */
    fun resolveProperty(expr: PsiElement): JuxPropertyDeclaration? {
        if (expr.elementType !in RECEIVERS) return null
        val nameLeaf = lastIdentifier(expr) ?: return null

        // Direct in-file resolution (bare names + this.Name).
        (expr.references.firstOrNull() as? JuxReference)?.resolveLocally()?.let {
            return it as? JuxPropertyDeclaration
        }

        // Qualified `q.Name`: type the qualifier, then find the member.
        if (expr.elementType !== E.FIELD_ACCESS_EXPRESSION) return null
        val qualifier = expr.firstChild ?: return null
        if (qualifier.elementType !== E.REFERENCE_EXPRESSION) return null
        val qDecl = (qualifier.references.firstOrNull() as? JuxReference)?.resolveLocally() ?: return null
        val qType = qDecl.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return null
        // Use the head type name (strip generics/nullability noise).
        val typeName = firstIdentifier(qType)?.text ?: return null
        val typeDecl = JuxTypeIndex.findType(expr.project, typeName) ?: return null
        return propertyMember(typeDecl, nameLeaf.text)
    }

    /** The property named [name] declared directly on [type], or null. */
    fun propertyMember(type: JuxTypeDeclaration, name: String): JuxPropertyDeclaration? {
        val body = type.node.findChildByType(E.CLASS_BODY)?.psi ?: return null
        return body.children.firstOrNull {
            it is JuxPropertyDeclaration && it.name == name
        } as? JuxPropertyDeclaration
    }

    /** Whitespace-stripped receiver chain text — the E0973 comparison key. */
    fun chainText(expr: PsiElement): String = expr.text.replace(WS, "")

    /** The last IDENTIFIER leaf directly under [parent], or null. */
    fun lastIdentifier(parent: PsiElement): PsiElement? {
        var last: PsiElement? = null
        var c: PsiElement? = parent.firstChild
        while (c != null) {
            if (c.elementType === JuxTokenTypes.IDENTIFIER) last = c
            c = c.nextSibling
        }
        return last
    }

    private fun firstIdentifier(parent: PsiElement): PsiElement? {
        var c: PsiElement? = parent.firstChild
        while (c != null) {
            if (c.elementType === JuxTokenTypes.IDENTIFIER) return c
            c = c.nextSibling
        }
        return null
    }

    private val RECEIVERS = setOf(E.REFERENCE_EXPRESSION, E.FIELD_ACCESS_EXPRESSION)
    private val WS = Regex("\\s+")
}
