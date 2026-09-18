package dev.jux.intellij.completion

import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.icons.AllIcons
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.PsiComment
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement

/**
 * Names for a declaration being written, as Java suggests them: with the
 * caret after the type of a local, field or parameter (`Vec<String> |`,
 * `HttpClient |`), the popup offers names made from the type rather than
 * the names already in scope, which could not go there.
 *
 *  - A type's camel-case words, longest first: `HttpClient` gives
 *    `httpClient`, then `client`.
 *  - A type holding one element type, and an array, is named for its
 *    elements first: `Vec<String>` gives `strings`, then `vec`; `int[]`
 *    gives `ints`.
 *  - A primitive gets its initial: `int` gives `i`.
 *  - A name that is a keyword is prefixed (`Type` gives `aType`), and one
 *    already taken in the enclosing function gets a number.
 */
object JuxNameSuggestions {

    /** Declarations whose name follows their type directly. */
    private val NAMED_AFTER_TYPE = setOf(E.LOCAL_VARIABLE, E.FIELD_DECLARATION, E.PARAMETER)

    /** Built-in value types, named by their initial. */
    private val PRIMITIVES = setOf(
        "int", "long", "short", "byte", "char", "float", "double", "bool",
        "uint", "ulong", "ushort", "ubyte", "isize", "usize",
    )

    /**
     * True when [position] is the name of a declaration right after its type:
     * the identifier being completed sits in a local, field or parameter, and
     * the nearest thing before it is that declaration's type.
     */
    fun isDeclarationName(position: PsiElement): Boolean {
        if (position.elementType !== T.IDENTIFIER) return false
        val decl = position.parent ?: return false
        if (decl.elementType !in NAMED_AFTER_TYPE) return false
        val type = previousMeaningful(position) ?: return false
        if (type.elementType !== E.TYPE_REFERENCE) return false
        return looksLikeType(type)
    }

    /**
     * The name items for the declaration at [position], most fitting first.
     * Each carries a descending tier so the sorter keeps this order when
     * nothing else separates them.
     */
    fun suggest(position: PsiElement): List<LookupElement> {
        val type = previousMeaningful(position) ?: return emptyList()
        val taken = namesInScope(position)
        val out = ArrayList<LookupElement>()
        for ((i, raw) in namesFor(type.text).withIndex()) {
            val name = unique(escape(raw), taken)
            val builder = LookupElementBuilder.create(name).withIcon(AllIcons.Nodes.Variable)
            builder.putUserData(JuxCompletionRanking.KIND, JuxCompletionRanking.Kind.LOCAL)
            builder.putUserData(JuxCompletionRanking.TIER, 100.0 - i)
            out.add(builder)
        }
        return out
    }

    /**
     * The raw names for a written type, most fitting first, before keyword
     * escaping and de-duplication. Public for its tests.
     */
    fun namesFor(typeText: String): List<String> {
        val text = typeText.replace(Regex("\\s+"), "").removeSuffix("?")
        val out = LinkedHashSet<String>()
        if (text.endsWith("[]")) {
            // An array is named for what it holds.
            val element = text.removeSuffix("[]").removeSuffix("?")
            elementNames(element).forEach { out.add(plural(it)) }
            return out.toList()
        }
        val base = text.substringBefore('<').substringAfterLast('.').substringAfterLast("::")
        val args = typeArguments(text)
        if (args.size == 1) {
            // A container of one element type: named for its elements first.
            elementNames(args[0]).forEach { out.add(plural(it)) }
        }
        if (base in PRIMITIVES) {
            out.add(base.take(1))
        } else {
            out.addAll(camelSuffixes(base))
        }
        return out.toList()
    }

    /**
     * What one element of a container is called, before pluralizing: a
     * primitive by its full name (`int[]` gives `ints`, not `is`), anything
     * else as a single value would be.
     */
    private fun elementNames(text: String): List<String> {
        val bare = text.trim().removeSuffix("?")
        return if (bare in PRIMITIVES) listOf(bare) else namesFor(bare)
    }

    /** `HttpClient` gives `httpClient`, `client`; `HTTPServer` gives `httpServer`, `server`. */
    fun camelSuffixes(name: String): List<String> {
        if (name.isEmpty()) return emptyList()
        val words = name.split(Regex("(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])")).filter { it.isNotEmpty() }
        return words.indices.map { start ->
            words.drop(start).mapIndexed { i, w -> if (i == 0) w.lowercase() else w.replaceFirstChar { it.uppercase() } }
                .joinToString("")
        }
    }

    /** English plural of a name's last word: `string` gives `strings`, `entry` gives `entries`. */
    fun plural(name: String): String = when {
        name.endsWith("s") || name.endsWith("x") || name.endsWith("sh") || name.endsWith("ch") -> name + "es"
        name.endsWith("y") && name.length > 1 && name[name.length - 2] !in "aeiou" -> name.dropLast(1) + "ies"
        else -> name + "s"
    }

    /** The top-level type arguments of `Name<A, B<C>>`: `A`, `B<C>`. */
    private fun typeArguments(text: String): List<String> {
        val open = text.indexOf('<')
        if (open < 0 || !text.endsWith(">")) return emptyList()
        val inner = text.substring(open + 1, text.length - 1)
        val out = ArrayList<String>()
        var depth = 0
        var start = 0
        for ((i, c) in inner.withIndex()) {
            when (c) {
                '<' -> depth++
                '>' -> depth--
                ',' -> if (depth == 0) { out.add(inner.substring(start, i)); start = i + 1 }
            }
        }
        out.add(inner.substring(start))
        // A wildcard names nothing; `? extends Shape` is named for `Shape`.
        return out.map { it.removePrefix("?").removePrefix("extends").removePrefix("super") }.filter { it.isNotEmpty() }
    }

    /** A keyword cannot be a name: `type` becomes `aType`. */
    private fun escape(name: String): String =
        if (T.keywordType(name) != null || name == "true" || name == "false" || name == "null") {
            "a" + name.replaceFirstChar { it.uppercase() }
        } else {
            name
        }

    /** [name], or `name1`, `name2`... when it is already declared nearby. */
    private fun unique(name: String, taken: Set<String>): String {
        if (name !in taken) return name
        var n = 1
        while ("$name$n" in taken) n++
        return "$name$n"
    }

    /**
     * Names already declared in the enclosing function (its parameters and
     * locals) or, for a field, the enclosing class body. Bounded to that one
     * declaration, never the file.
     */
    private fun namesInScope(position: PsiElement): Set<String> {
        val self = position.parent
        var scope: PsiElement? = self?.parent
        while (scope != null && scope.elementType !in SCOPES) scope = scope.parent
        if (scope == null) return emptySet()
        val out = HashSet<String>()
        val named = if (scope.elementType === E.CLASS_BODY) scope.children.toList()
        else PsiTreeUtil.findChildrenOfType(scope, JuxNamedElement::class.java).filter {
            it.elementType === E.LOCAL_VARIABLE || it.elementType === E.PARAMETER
        }
        for (d in named) {
            if (d == self) continue
            (d as? JuxNamedElement)?.name?.let { out.add(it) }
        }
        return out
    }

    private val SCOPES = setOf(
        E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION,
        E.LAMBDA_EXPRESSION, E.CLASS_BODY,
    )

    /** The sibling before [e], skipping whitespace, comments and error nodes. */
    private fun previousMeaningful(e: PsiElement): PsiElement? {
        var p = e.prevSibling
        while (p != null && (p is PsiWhiteSpace || p is PsiComment || p is com.intellij.psi.PsiErrorElement)) p = p.prevSibling
        return p
    }

    /**
     * Whether a TYPE_REFERENCE reads as a type rather than a mis-parsed
     * statement (`foo bar`): a built-in type, a capitalised name, or anything
     * with type arguments or array brackets.
     */
    private fun looksLikeType(type: PsiElement): Boolean {
        val text = type.text.trim()
        if (text.isEmpty()) return false
        val base = text.substringBefore('<').substringBefore('[').removeSuffix("?").substringAfterLast('.')
        return base in PRIMITIVES || base == "String" || base.firstOrNull()?.isUpperCase() == true ||
            text.contains('<') || text.contains('[')
    }
}
