package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeEngine
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * **E0443** for a Jux type written with the wrong number of type arguments,
 * at any depth (ERRATA E135): `Vec<Vec<Pair<int>>>` is caught at the
 * innermost `Pair<int>` for a `Pair<K, V>`, in every place a type is written
 * -- a field, a parameter, a return, a local, a `new`, a supertype, a bound.
 *
 * Mirrors the checker's `check_type_arity`: only a Jux class, interface,
 * record or enum is held to its count. A Rust type may leave defaulted
 * parameters unwritten (§T.4.7.1), a `type` alias is checked where it is
 * used ([JuxTypeAliasInspection]), a type parameter takes none, and a type
 * written with no arguments at all is left to inference (the diamond).
 *
 * The parser keeps only the first level of a type's arguments as PSI, so the
 * written text is walked instead, from the outermost reference.
 */
class JuxTypeArgumentCountInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor {
        if (DumbService.isDumb(holder.project)) return PsiElementVisitor.EMPTY_VISITOR
        if (holder.file.name.endsWith(".jux.d")) return PsiElementVisitor.EMPTY_VISITOR
        return object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType === E.TYPE_REFERENCE) check(element, holder)
            }
        }
    }

    private fun check(ref: PsiElement, holder: ProblemsHolder) {
        // Only the outermost reference: a nested one is part of its text.
        var p = ref.parent
        while (p != null && p !is com.intellij.psi.PsiFile) {
            val t = p.elementType
            if (t === E.TYPE_REFERENCE || t === E.TYPE_ARGUMENT_LIST || t === E.WILDCARD_TYPE) return
            if (t === E.TYPE_PARAMETER_LIST || t in STOP) break
            p = p.parent
        }
        // A bound in `<T extends Vec<Pair<K, V>>>` is a bare name whose
        // arguments follow it as tokens of the parameter list.
        val anchor = if (ref.parent?.elementType === E.TYPE_PARAMETER_LIST) ref.parent else ref
        val fileText = ref.containingFile?.text ?: return
        val start = ref.textRange.startOffset
        val end = anchor.textRange.endOffset
        val node = TypeTextParser(fileText, start, end).parse() ?: return
        visit(node, ref, anchor, holder)
    }

    private fun visit(node: TypeNode, ref: PsiElement, anchor: PsiElement, holder: ProblemsHolder) {
        node.args.forEach { visit(it, ref, anchor, holder) }
        val name = node.name ?: return
        if (node.args.isEmpty()) return
        val simple = name.substringAfterLast('.')
        val qualifier = name.substringBeforeLast('.', "").ifEmpty { null }
        val decl = JuxTypeEngine.resolveTypeName(ref, simple, qualifier) as? JuxTypeDeclaration ?: return
        if (!heldToCount(decl)) return
        val params = JuxHierarchy.typeParameterNames(decl)
        val written = node.args.size
        if (written == params.size) return
        // A name this file neither declares nor settles by import may mean
        // another same-named type (`Box` under `import rust.std.*;` when the
        // stub is not indexed, beside a stranger's `class Box`): when one of
        // them takes the written count, which is meant is not certain.
        if (decl.containingFile != ref.containingFile &&
            JuxTypeIndex.typesNamed(ref.project, simple).any {
                it !== decl && JuxHierarchy.typeParameterNames(it).size == written
            }
        ) return
        val shown = node.text
        val message = if (params.isEmpty()) {
            "`$simple` is not generic, but `$shown` gives it $written type argument${if (written == 1) "" else "s"} (E0443)"
        } else {
            val list = params.joinToString(", ") { "`$it`" }
            val were = if (written == 1) "1 was" else "$written were"
            "`$simple` takes ${params.size} type argument${if (params.size == 1) "" else "s"} ($list), " +
                "but $were written in `$shown` (E0443)"
        }
        val range = TextRange(node.start, node.end).shiftLeft(anchor.textRange.startOffset)
        holder.registerProblem(anchor, message, ProblemHighlightType.GENERIC_ERROR, range)
    }

    /** A Jux class, interface, record, struct or enum; not an alias, not a Rust type. */
    private fun heldToCount(decl: JuxTypeDeclaration): Boolean {
        return decl.elementType in KINDS && !JuxHierarchy.isRustType(decl)
    }

    /** One written type: its name (null for a function type, a tuple, a literal) and its arguments. */
    private class TypeNode(val name: String?, val args: List<TypeNode>, val start: Int, val end: Int, val text: String)

    /**
     * A reader of a written type, `a.b.Name<Arg, ? extends X<Y>>[]?`, over
     * [text] from [from] up to [limit]. Function types, tuples and `const`
     * arguments are read past, their inner types included.
     */
    private class TypeTextParser(val text: String, from: Int, val limit: Int) {
        var i = from

        fun parse(): TypeNode? = try { node() } catch (_: IndexOutOfBoundsException) { null }

        private fun ws() { while (i < limit && text[i].isWhitespace()) i++ }

        private fun node(): TypeNode? {
            ws()
            if (i >= limit) return null
            val start = i
            val c = text[i]
            when {
                c == '?' -> {
                    i++; ws()
                    if (text.startsWith("extends", i) || text.startsWith("super", i)) {
                        while (i < limit && text[i].isLetter()) i++
                        return node()
                    }
                    return TypeNode(null, emptyList(), start, i, "?")
                }
                c == '(' -> {
                    // A function type `(A, B) -> R` or a tuple `(A, B)`: its
                    // parts are types too.
                    val inner = ArrayList<TypeNode>()
                    i++
                    while (i < limit && text[i] != ')') {
                        node()?.let { inner.add(it) }
                        ws()
                        if (i < limit && text[i] == ',') i++ else if (i < limit && text[i] != ')') return null
                    }
                    i++ // `)`
                    ws()
                    if (text.startsWith("->", i)) { i += 2; node()?.let { inner.add(it) } }
                    return TypeNode(null, inner, start, i, text.substring(start, i))
                }
                c.isLetter() || c == '_' -> {
                    while (i < limit && (text[i].isLetterOrDigit() || text[i] == '_' || text[i] == '.')) i++
                    val name = text.substring(start, i).trimEnd('.')
                    val args = ArrayList<TypeNode>()
                    val save = i
                    ws()
                    if (i < limit && text[i] == '<') {
                        i++
                        ws()
                        while (i < limit && text[i] != '>') {
                            args.add(node() ?: return null)
                            ws()
                            if (i < limit && text[i] == ',') i++ else if (i < limit && text[i] != '>') return null
                        }
                        if (i >= limit) return null
                        i++ // `>`
                    } else {
                        i = save
                    }
                    val end = i
                    // Suffixes: `[]`, `[N]`, `?`, `*`.
                    while (i < limit && (text[i] == '[' || text[i] == '?' || text[i] == '*')) {
                        if (text[i] == '[') { while (i < limit && text[i] != ']') i++ }
                        i++
                    }
                    return TypeNode(name, args, start, end, text.substring(start, end))
                }
                c.isDigit() || c == '-' -> {
                    // A `const` argument: `Chunk<int, 3>`.
                    i++
                    while (i < limit && (text[i].isLetterOrDigit() || text[i] == '_')) i++
                    return TypeNode(null, emptyList(), start, i, text.substring(start, i))
                }
                else -> return null
            }
        }
    }

    private companion object {
        val KINDS = setOf(
            E.CLASS_DECLARATION, E.INTERFACE_DECLARATION, E.RECORD_DECLARATION,
            E.STRUCT_DECLARATION, E.ENUM_DECLARATION,
        )

        /** Nodes past which a reference is not part of an enclosing written type. */
        val STOP = setOf(
            E.CALL_EXPRESSION, E.NEW_EXPRESSION, E.CAST_EXPRESSION, E.CODE_BLOCK, E.CLASS_BODY,
            E.PARAMETER, E.FIELD_DECLARATION, E.METHOD_DECLARATION, E.LOCAL_VARIABLE,
        )
    }
}
