package dev.jux.intellij.refactoring

import com.intellij.openapi.editor.Document
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.codeStyle.CodeStyleManager
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.search.searches.ReferencesSearch
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * Tree and edit helpers the method-level refactorings share (Extract Method,
 * Inline Method, Change Signature, Introduce Parameter, Introduce Parameter
 * Object, Pull Up / Push Down).
 *
 * Every refactoring here edits TEXT through the document, never PSI: the Jux
 * tree is a lenient parser's output with no element factory for most shapes,
 * and a text edit followed by a reparse and a reformat is both simpler and the
 * thing the user can predict. [Edit] + [applyEdits] is the one write path.
 */
internal object JuxRefactoringUtil {

    /**
     * One replacement in one file. Its new text is a sequence of [Part]s:
     * literal text, and [Part.Slice]s of the ORIGINAL file, so an edit that
     * moves code (reordered arguments, a statement into a new method) says so.
     *
     * That is what lets edits nest. Reordering `f(n - 1)` while renaming `n`
     * gives an argument-list edit and a rename inside one of its arguments;
     * as plain strings they overlap and one clobbers the other. As slices,
     * the rename is applied inside the slice it falls in.
     */
    data class Edit(val file: PsiFile, val range: TextRange, val parts: List<Part>) {
        constructor(file: PsiFile, range: TextRange, text: String) : this(file, range, listOf(Part.Lit(text)))
    }

    /** A piece of an [Edit]'s new text. */
    sealed class Part {
        data class Lit(val text: String) : Part()
        data class Slice(val range: TextRange) : Part()
    }

    /**
     * The final (range, text) replacements for one file: nested edits applied
     * into the slices of the edit containing them, the outermost ones left.
     * An edit inside another edit's range but in none of its slices was
     * replaced wholesale and is dropped.
     */
    private fun flatten(fileText: String, edits: List<Edit>): List<Pair<TextRange, String>> {
        val flattener = Flattener(fileText)
        // Two insertions at one offset (two inlined calls in one statement each
        // adding lines before it) are one insertion of both, in order.
        val (inserts, replacements) = edits.distinctBy { it.range to it.parts }.partition { it.range.isEmpty }
        val merged = inserts.groupBy { it.range.startOffset }.map { (_, group) ->
            Edit(group.first().file, group.first().range, group.flatMap { it.parts })
        }
        val unique = replacements + merged
        return flattener.maximal(null, unique).map { it.range to flattener.render(it, unique) }
    }

    /** The recursion behind [flatten]: an edit's slices render with the edits nested in them. */
    private class Flattener(private val fileText: String) {
        // An insertion at the very edge of a replaced range is beside it, not in it.
        private fun strictlyInside(outer: TextRange, e: Edit) =
            outer.contains(e.range) && outer != e.range &&
                !(e.range.isEmpty && (e.range.startOffset == outer.startOffset || e.range.startOffset == outer.endOffset))

        /** The outermost edits of [pool] within [within] (all of them for null), in order. */
        fun maximal(within: TextRange?, pool: List<Edit>): List<Edit> {
            val inside = if (within == null) pool else pool.filter { within.contains(it.range) }
            return inside.filter { e -> inside.none { o -> o !== e && strictlyInside(o.range, e) } }
                .distinctBy { it.range }
                .sortedBy { it.range.startOffset }
        }

        /** [edit]'s new text, with the edits of [pool] that fall in its slices applied. */
        fun render(edit: Edit, pool: List<Edit>): String {
            val nested = pool.filter { it !== edit && strictlyInside(edit.range, it) }
            return edit.parts.joinToString("") { part ->
                when (part) {
                    is Part.Lit -> part.text
                    is Part.Slice -> renderRange(part.range, nested)
                }
            }
        }

        /** The original text of [range] with the outermost edits of [pool] inside it applied. */
        private fun renderRange(range: TextRange, pool: List<Edit>): String {
            val sb = StringBuilder()
            var at = range.startOffset
            for (e in maximal(range, pool)) {
                if (e.range.startOffset < at) continue // overlapping, not nested: the first one wins
                sb.append(fileText, at, e.range.startOffset)
                sb.append(render(e, pool))
                at = e.range.endOffset
            }
            sb.append(fileText, at, range.endOffset)
            return sb.toString()
        }
    }

    /** A flattened replacement, ready for the document. */
    private data class Flat(val range: TextRange, val text: String)

    /**
     * Apply [edits] in one go: grouped per file, nested ones merged into the
     * edit that contains them, applied back to front so earlier offsets stay
     * valid, then committed and the touched ranges reformatted. Must run inside
     * a write command.
     */
    fun applyEdits(project: Project, edits: List<Edit>, reformat: Boolean = true) {
        val manager = PsiDocumentManager.getInstance(project)
        for ((file, rawEdits) in edits.groupBy { it.file }) {
            val fileText = manager.getDocument(file)?.text ?: continue
            val fileEdits = flatten(fileText, rawEdits).map { (range, text) -> Flat(range, text) }
            val document = manager.getDocument(file) ?: continue
            manager.doPostponedOperationsAndUnblockDocument(document)
            // Replaced ranges, tracked by marker so the reformat below hits
            // what was written even after later edits shift it.
            val markers = ArrayList<com.intellij.openapi.editor.RangeMarker>()
            // Back to front; at one offset the replacement goes first, so an
            // insertion there lands in front of the new text, not inside it.
            val ordered = fileEdits.sortedWith(
                compareByDescending<Flat> { it.range.startOffset }.thenBy { if (it.range.isEmpty) 1 else 0 },
            )
            for (edit in ordered) {
                document.replaceString(edit.range.startOffset, edit.range.endOffset, edit.text)
                markers.add(document.createRangeMarker(edit.range.startOffset, edit.range.startOffset + edit.text.length))
            }
            manager.commitDocument(document)
            if (reformat) {
                val fresh = manager.getPsiFile(document) ?: continue
                for (marker in markers) {
                    if (marker.isValid && marker.endOffset > marker.startOffset) {
                        CodeStyleManager.getInstance(project).reformatText(fresh, marker.startOffset, marker.endOffset)
                    }
                    marker.dispose()
                }
                manager.commitDocument(document)
            }
        }
    }

    /** The document behind [file], for callers that need offsets of their own. */
    fun documentOf(file: PsiFile): Document? = PsiDocumentManager.getInstance(file.project).getDocument(file)

    // ---- callables -------------------------------------------------------

    /** Element types that own a parameter list and a body. */
    val CALLABLE_KINDS = setOf(E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION)

    /** The method, constructor or operator [element] is inside, or null. */
    fun enclosingCallable(element: PsiElement): PsiElement? {
        var e: PsiElement? = element.parent
        while (e != null && e !is PsiFile) {
            if (e.elementType in CALLABLE_KINDS) return e
            e = e.parent
        }
        return null
    }

    /** True when [callable] is a free function at the top of a file. */
    fun isTopLevel(callable: PsiElement): Boolean = callable.parent is JuxFile

    /** True when code in [callable] has no `this`: a static member or a free function. */
    fun isStaticContext(callable: PsiElement): Boolean =
        isTopLevel(callable) || JuxHierarchy.hasModifier(callable, "static")

    /** The callable's body block, or null for an abstract/interface method. */
    fun body(callable: PsiElement): PsiElement? = callable.node.findChildByType(E.CODE_BLOCK)?.psi

    /** The `(…)` parameter list of [callable]. */
    fun parameterList(callable: PsiElement): PsiElement? = callable.node.findChildByType(E.PARAMETER_LIST)?.psi

    /** The declared type text of a parameter, local, field or record component, or null for `var`. */
    fun declaredTypeText(declaration: PsiElement): String? =
        declaration.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()

    // ---- types -----------------------------------------------------------

    /**
     * The type [expression] should be written as in a declaration, or null
     * when it cannot be known.
     *
     * What the source states outright comes first ([JuxExtractSupport.declaredTypeText]:
     * literals, `new T(...)`, casts, names with a written type); the plugin's
     * type engine answers the rest (arithmetic, comparisons, calls, `var`
     * chains). A type the engine only partly knows (`Vec<?>`) is NOT written:
     * a guessed type compiles into a different program instead of asking.
     */
    fun typeTextOf(expression: PsiElement): String? {
        JuxExtractSupport.declaredTypeText(expression)?.let { return it }
        return writable(dev.jux.intellij.resolve.JuxTypeEngine.typeOf(expression))
    }

    /** The type a value declaration has, written, or null when unknown. */
    fun typeTextOfDeclaration(declaration: PsiElement): String? {
        declaredTypeText(declaration)?.takeIf { it != "var" }?.let { return it }
        return writable(dev.jux.intellij.resolve.JuxTypeEngine.declaredType(declaration))
    }

    /** [type]'s source spelling, or null when any part of it is unknown. */
    private fun writable(type: dev.jux.intellij.resolve.JuxType): String? {
        fun known(t: dev.jux.intellij.resolve.JuxType): Boolean = when (t) {
            is dev.jux.intellij.resolve.JuxType.ClassType -> t.decl.name != null && t.args.all { known(it) }
            is dev.jux.intellij.resolve.JuxType.ArrayType -> known(t.element)
            is dev.jux.intellij.resolve.JuxType.Nullable -> known(t.inner)
            is dev.jux.intellij.resolve.JuxType.Primitive -> true
            is dev.jux.intellij.resolve.JuxType.TypeVar -> t.param.name != null
            else -> false
        }
        return if (known(type)) type.presentable() else null
    }

    // ---- calls -----------------------------------------------------------

    /**
     * Every in-project call of [callable], as its CALL_EXPRESSION.
     *
     * Found the way Find Usages finds them (a word-index search whose hits are
     * resolved by the plugin's own reference resolution), so a call the IDE
     * cannot resolve is not rewritten. That is the conservative side: a missed
     * call site fails to compile loudly, a wrongly rewritten one might not.
     */
    fun callsOf(callable: PsiElement): List<PsiElement> {
        val out = LinkedHashSet<PsiElement>()
        val scope = GlobalSearchScope.projectScope(callable.project)
        for (ref in ReferencesSearch.search(callable, scope).findAll()) {
            val element = ref.element
            val call = element.parent
            if (call?.elementType === E.CALL_EXPRESSION && call.firstChild === element) out.add(call)
        }
        return out.sortedWith(compareBy({ it.containingFile.name }, { it.textRange.startOffset }))
    }

    /** The argument list of a call. */
    fun argumentList(call: PsiElement): PsiElement? = call.node.findChildByType(E.ARGUMENT_LIST)?.psi

    /** The argument expressions of a call, in order. */
    fun arguments(call: PsiElement): List<PsiElement> =
        argumentList(call)?.children?.filter { isExpression(it) } ?: emptyList()

    /** True for an expression node (anything the parser produced as one). */
    fun isExpression(element: PsiElement): Boolean {
        val t = element.elementType ?: return false
        return t.toString().endsWith("_EXPRESSION")
    }

    /**
     * The receiver text a call names, `a` in `a.m()`, or null for a bare `m()`.
     * `this.m()` gives `this`.
     */
    fun receiverText(call: PsiElement): String? {
        val callee = call.firstChild ?: return null
        if (callee.elementType !== E.FIELD_ACCESS_EXPRESSION) return null
        return callee.firstChild?.text
    }

    // ---- statements ------------------------------------------------------

    /** The statements of a code block, without the braces, whitespace and comments. */
    fun statements(block: PsiElement): List<PsiElement> =
        block.children.filter { it !is PsiWhiteSpace && it !is PsiComment && !isBrace(it) }

    private fun isBrace(e: PsiElement) =
        e.elementType === JuxTokenTypes.LBRACE || e.elementType === JuxTokenTypes.RBRACE

    /** Every descendant of [root] (inclusive) of element type [type]. */
    fun descendants(root: PsiElement, type: com.intellij.psi.tree.IElementType): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        root.accept(object : com.intellij.psi.PsiRecursiveElementWalkingVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType === type) out.add(element)
                super.visitElement(element)
            }
        })
        return out
    }

    /**
     * Every read or write of a local or parameter under [root], as
     * (declaration, use site) pairs in source order.
     *
     * A name inside an interpolated string (`$"hi ${name}"`, `$name`) is a use
     * too, but the string is one token, so it has no reference of its own: it
     * is resolved by scope from the token's position. Missing those made
     * Extract Method drop a parameter the moved code still printed.
     */
    fun variableUses(root: PsiElement): List<Pair<PsiElement, PsiElement>> {
        val out = ArrayList<Pair<PsiElement, PsiElement>>()
        root.accept(object : com.intellij.psi.PsiRecursiveElementWalkingVisitor() {
            override fun visitElement(element: PsiElement) {
                when (element.elementType) {
                    E.REFERENCE_EXPRESSION -> resolve(element)?.takeIf { isVariable(it) }?.let { out.add(it to element) }
                    JuxTokenTypes.INTERP_STRING_LITERAL, JuxTokenTypes.INTERP_RAW_STRING_LITERAL -> {
                        val raw = element.elementType === JuxTokenTypes.INTERP_RAW_STRING_LITERAL
                        for (name in dev.jux.intellij.editor.JuxImportSupport.interpolatedNames(element.text, raw)) {
                            visibleVariable(name, element)?.let { out.add(it to element) }
                        }
                    }
                }
                super.visitElement(element)
            }
        })
        return out
    }

    /**
     * Edits that rewrite each use of the declarations in [renames] (declaration
     * -> new text) under [root]. A plain reference is replaced; an interpolated
     * string gets ONE edit applying every rename that touches it, since two
     * edits on the same token would overwrite each other.
     */
    fun renameEdits(root: PsiElement, renames: Map<PsiElement, String>): List<Edit> {
        val file = root.containingFile
        val out = ArrayList<Edit>()
        val tokens = LinkedHashMap<PsiElement, MutableList<PsiElement>>()
        for ((decl, use) in variableUses(root)) {
            val target = renames[decl] ?: continue
            if (use.elementType === E.REFERENCE_EXPRESSION) out += Edit(file, use.textRange, target)
            else tokens.getOrPut(use) { ArrayList() }.add(decl)
        }
        for ((token, decls) in tokens) {
            val raw = token.elementType === JuxTokenTypes.INTERP_RAW_STRING_LITERAL
            val byName = decls.distinct().mapNotNull { d -> nameOf(d)?.let { it to renames.getValue(d) } }.toMap()
            out += Edit(file, token.textRange, JuxChangeSignature.renameInInterpolation(token.text, byName, raw))
        }
        return out
    }

    /**
     * The range to delete to remove a member or a top-level declaration: the
     * doc comment right above it, its lines, and one of the blank lines around
     * it, so what is left stays one blank line apart.
     */
    fun memberDeletionRange(member: PsiElement): TextRange {
        var start: PsiElement = member
        var prev = member.prevSibling
        while (prev is PsiWhiteSpace && !prev.text.contains("\n\n")) {
            val before = prev.prevSibling
            if (before is PsiComment) {
                start = before; prev = before.prevSibling
            } else break
        }
        val text = member.containingFile.text
        var s = start.textRange.startOffset
        while (s > 0 && (text[s - 1] == ' ' || text[s - 1] == '\t')) s--
        // One blank line above goes with it.
        var s2 = s
        if (s2 > 0 && text[s2 - 1] == '\n') {
            var k = s2 - 1
            while (k > 0 && (text[k - 1] == ' ' || text[k - 1] == '\t')) k--
            if (k > 0 && text[k - 1] == '\n') s2 = k
        }
        var e = member.textRange.endOffset
        while (e < text.length && (text[e] == ' ' || text[e] == '\t')) e++
        if (e < text.length && text[e] == '\n') e++
        if (s2 == s) {
            // No blank line went from above, so take the one below.
            var k = e
            while (k < text.length && (text[k] == ' ' || text[k] == '\t')) k++
            if (k < text.length && text[k] == '\n') e = k + 1
        }
        return TextRange(s2, e)
    }

    /** A local or a parameter. */
    fun isVariable(decl: PsiElement): Boolean =
        decl is dev.jux.intellij.psi.JuxLocalVariable || decl is dev.jux.intellij.psi.JuxParameter

    /**
     * The local or parameter [name] means at [at], by the same scope rules as
     * the plugin's reference resolution: a local is visible after its
     * declaration, inner scopes first.
     */
    fun visibleVariable(name: String, at: PsiElement): PsiElement? {
        val offset = at.textRange.startOffset
        var scope: PsiElement? = at.parent
        while (scope != null && scope !is PsiFile) {
            val found = when (scope.elementType) {
                E.CODE_BLOCK -> scope.children.lastOrNull {
                    it.elementType === E.LOCAL_VARIABLE && it.textRange.endOffset <= offset && nameOf(it) == name
                }
                E.FOR_EACH_STATEMENT, E.FOR_STATEMENT, E.CATCH_CLAUSE ->
                    scope.children.firstOrNull { it.elementType === E.LOCAL_VARIABLE && nameOf(it) == name }
                E.LAMBDA_EXPRESSION, E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION -> {
                    val list = scope.children.firstOrNull { it.elementType === E.PARAMETER_LIST }
                    ((list?.children?.toList() ?: emptyList()) + scope.children)
                        .firstOrNull { it.elementType === E.PARAMETER && nameOf(it) == name }
                }
                else -> null
            }
            if (found != null) return found
            scope = scope.parent
        }
        return null
    }

    /** What a bare name reference resolves to, through the plugin's references. */
    fun resolve(reference: PsiElement): PsiElement? =
        reference.references.firstNotNullOfOrNull { it.resolve() }

    /** The name a named declaration carries. */
    fun nameOf(element: PsiElement?): String? = (element as? JuxNamedElement)?.name

    /** The enclosing type of [element], or null at the top of a file. */
    fun enclosingType(element: PsiElement): JuxTypeDeclaration? = JuxHierarchy.enclosingType(element)

    /**
     * The text of [element] re-indented so its first line starts at column 0:
     * the common leading whitespace of its continuation lines is removed. The
     * reformat after insertion puts it back at the right depth.
     */
    fun dedent(text: String): String {
        val lines = text.lines()
        if (lines.size < 2) return text
        val indent = lines.drop(1).filter { it.isNotBlank() }.minOfOrNull { l -> l.takeWhile { it == ' ' || it == '\t' }.length } ?: 0
        return (listOf(lines.first()) + lines.drop(1).map { if (it.length >= indent) it.substring(indent) else it.trimStart() })
            .joinToString("\n")
    }

    /**
     * A member name like [base] that the type body (or the file, for top-level
     * functions) does not already declare.
     */
    fun uniqueMemberName(base: String, container: PsiElement): String {
        val taken = container.children.mapNotNull { nameOf(it) }.toSet()
        if (base !in taken) return base
        var i = 1
        while ("$base$i" in taken) i++
        return "$base$i"
    }
}
