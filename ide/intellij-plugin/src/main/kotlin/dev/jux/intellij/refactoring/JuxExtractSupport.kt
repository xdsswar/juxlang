package dev.jux.intellij.refactoring

import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeInference

/**
 * The analysis every Jux extract-style refactoring shares: find the expression
 * the user means, find its other occurrences, find where a declaration for it
 * would go, and guess its type.
 *
 * Deliberately analysis-only. Each handler does its own single document edit,
 * so this file has no mutation in it and can be reasoned about (and tested) as
 * pure functions over the tree.
 */
internal object JuxExtractSupport {

    /**
     * The expression to extract: the selected one, or — with no selection — the
     * innermost expression containing the caret.
     *
     * Null when the caret is not in an expression, or is in one whose
     * extraction would change behaviour (see [hasSideEffects]).
     */
    fun expressionAt(file: PsiFile, editor: Editor): PsiElement? {
        val selection = editor.selectionModel
        val expression = if (selection.hasSelection()) {
            exactExpression(file, selection.selectionStart, selection.selectionEnd)
        } else {
            innermostExpression(file, editor.caretModel.offset)
        }
        return expression?.takeIf { !hasSideEffects(it) }
    }

    /**
     * The expression whose range is exactly `[start, end)`, if there is one.
     *
     * Several nodes can share that range — selecting `"hello"` matches both the
     * string TOKEN and the LITERAL_EXPRESSION wrapping it — so the walk keeps
     * going and returns the outermost match. Stopping at the first one returned
     * a bare token, which is not an expression, and the refactoring declined a
     * perfectly good selection.
     */
    private fun exactExpression(file: PsiFile, start: Int, end: Int): PsiElement? {
        var e: PsiElement? = file.findElementAt(start) ?: return null
        var best: PsiElement? = null
        while (e != null) {
            val range = e.textRange
            if (range.startOffset == start && range.endOffset == end) {
                if (e.node?.elementType in EXPRESSION_KINDS) best = e
            } else if (range.startOffset < start || range.endOffset > end) {
                break
            }
            e = e.parent
        }
        return best
    }

    /** The innermost expression containing [offset]. */
    private fun innermostExpression(file: PsiFile, offset: Int): PsiElement? {
        val at = file.findElementAt(offset) ?: file.findElementAt((offset - 1).coerceAtLeast(0)) ?: return null
        var e: PsiElement? = at
        while (e != null && e !is PsiFile) {
            if (e.node?.elementType in EXPRESSION_KINDS) return e
            e = e.parent
        }
        return null
    }

    /**
     * Whether extracting [expression] would change *when* something happens.
     *
     * An assignment or an increment writes; a lambda body runs later, or more
     * than once. Hoisting any of those to a single evaluation before the
     * statement is a behaviour change, not a refactoring, so the handlers
     * refuse rather than quietly rewriting the program.
     */
    fun hasSideEffects(expression: PsiElement): Boolean {
        if (expression.node?.elementType === E.LAMBDA_EXPRESSION) return true
        var found = false
        expression.accept(object : com.intellij.psi.PsiRecursiveElementWalkingVisitor() {
            override fun visitElement(element: PsiElement) {
                when (element.node?.elementType) {
                    E.ASSIGNMENT_EXPRESSION, E.LAMBDA_EXPRESSION -> found = true
                    E.POSTFIX_EXPRESSION -> found = true
                    E.UNARY_EXPRESSION ->
                        if (INCREMENTS.any { element.text.startsWith(it) }) found = true
                }
                if (!found) super.visitElement(element)
            }
        })
        return found
    }

    /**
     * Every occurrence of the same expression within [scope], in source order.
     *
     * Compared by whitespace-normalized text, the same equivalence
     * `JuxPropertyUsages` uses for property chains. It is a syntactic test, so
     * two spellings of one value (`a.b` and `this.a.b`) are not merged — which
     * is the conservative direction: a missed occurrence leaves working code,
     * a wrong merge does not.
     */
    fun occurrencesOf(expression: PsiElement, scope: PsiElement): List<PsiElement> {
        val key = normalize(expression.text)
        val kind = expression.node?.elementType
        val out = ArrayList<PsiElement>()
        scope.accept(object : com.intellij.psi.PsiRecursiveElementWalkingVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.node?.elementType == kind && normalize(element.text) == key) {
                    out.add(element)
                    // Do not descend into a match: `a.b` inside `a.b` is the
                    // same text at a different level and would double-count.
                    return
                }
                super.visitElement(element)
            }
        })
        return out.sortedBy { it.textRange.startOffset }
    }

    /**
     * The block an extracted declaration should live in — the nearest enclosing
     * code block, or the file itself in script mode.
     */
    fun enclosingBlock(element: PsiElement): PsiElement? {
        var e: PsiElement? = element
        while (e != null) {
            if (e.node?.elementType === E.CODE_BLOCK) return e
            if (e.parent is PsiFile) return e.parent
            e = e.parent
        }
        return null
    }

    /**
     * The statement inside [block] that contains [element] — the line a new
     * declaration must be inserted before.
     */
    fun statementIn(block: PsiElement, element: PsiElement): PsiElement? {
        var e: PsiElement? = element
        while (e != null && e.parent != null) {
            if (e.parent === block) return e
            e = e.parent
        }
        return null
    }

    /** The enclosing type declaration's body, for a constant's home. */
    fun enclosingClassBody(element: PsiElement): PsiElement? {
        var e: PsiElement? = element
        while (e != null) {
            if (e.node?.elementType === E.CLASS_BODY) return e
            e = e.parent
        }
        return null
    }

    /**
     * The written type of [expression] when it can be read off the source, else
     * null.
     *
     * Only shapes that are certain from the text alone: a literal, or a
     * `new T(...)`. Anything else would need the type checker, and a guess put
     * into a field declaration compiles into a wrong program rather than
     * surfacing as a question — so the handler asks instead.
     */
    fun inferTypeText(expression: PsiElement): String? {
        if (expression.node?.elementType === E.NEW_EXPRESSION) {
            return expression.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()
        }
        val text = expression.text.trim()
        return when {
            text.startsWith("\"") || text.startsWith("\"\"\"") -> "String"
            text.startsWith("$\"") || text.startsWith("r\"") -> "String"
            text.startsWith("'") -> "char"
            text == "true" || text == "false" -> "bool"
            LONG_LITERAL.matches(text) -> "long"
            FLOAT_LITERAL.matches(text) -> "float"
            DOUBLE_LITERAL.matches(text) -> "double"
            INT_LITERAL.matches(text) -> "int"
            else -> null
        }
    }

    /**
     * The type an extracted expression should be **declared** with, or null
     * when it genuinely cannot be read off the source.
     *
     * [inferTypeText] answers for literals and `new T(...)`; this adds the
     * shapes that make up most real constants: a name whose declaration is in
     * scope, a call whose return type is written down, a member access, a cast,
     * and parentheses around any of them. Generic arguments come through
     * whole — `Map<String, Leaf>`, not `Map` — because a field declaration has
     * to spell the type out.
     *
     * Anything left (arithmetic, a ternary, a chain through a `var`) still
     * yields null and the handler refuses, because a guessed type in a field
     * declaration does not surface as a question, it compiles into a different
     * program.
     */
    fun declaredTypeText(expression: PsiElement): String? {
        inferTypeText(expression)?.let { return it }
        val text = when (expression.node?.elementType) {
            E.PARENTHESIZED_EXPRESSION -> innerExpression(expression)?.let { declaredTypeText(it) }
            // A cast is the one shape that states its own type outright.
            E.CAST_EXPRESSION -> expression.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()
            E.REFERENCE_EXPRESSION ->
                JuxTypeInference.writtenTypeTextOf(expression.text.trim(), expression)

            E.CALL_EXPRESSION -> calleeTypeText(expression)
            E.FIELD_ACCESS_EXPRESSION -> memberTypeText(expression)
            else -> null
        }
        // `void` is a return type, not a type a value can have.
        return text?.takeIf { it.isNotBlank() && it != "void" }
    }

    /** The return type of the method a call expression names. */
    private fun calleeTypeText(call: PsiElement): String? {
        val callee = call.firstChild ?: return null
        return when (callee.node?.elementType) {
            // `recv.m(…)` — the member's own first TYPE_REFERENCE is its return type.
            E.FIELD_ACCESS_EXPRESSION -> memberTypeText(callee)
            // A bare `m(…)`: a member of the enclosing type, or a free function.
            else -> {
                val name = callee.text.trim()
                val onType = JuxHierarchy.enclosingType(call)
                    ?.let { JuxTypeInference.memberTypeTextOf(it, name) }
                onType ?: freeFunctionTypeText(call, name)
            }
        }
    }

    /** The declared type of `recv.member`, resolving `recv` in this file. */
    private fun memberTypeText(access: PsiElement): String? {
        val member = access.lastChild?.text?.trim() ?: return null
        val receiver = access.firstChild?.text?.trim() ?: return null
        val owner = JuxTypeInference.resolveReceiverExpression(receiver, access)?.type ?: return null
        return JuxTypeInference.memberTypeTextOf(owner, member)
    }

    /** The return type of a top-level function declared in the same file. */
    private fun freeFunctionTypeText(context: PsiElement, name: String): String? =
        context.containingFile?.children
            ?.firstOrNull {
                it.node?.elementType === E.METHOD_DECLARATION &&
                    (it as? JuxNamedElement)?.name == name
            }
            ?.let { JuxHierarchy.returnTypeText(it) }

    /** The expression inside a parenthesized expression, if any. */
    private fun innerExpression(parens: PsiElement): PsiElement? =
        parens.children.firstOrNull { it.node?.elementType in EXPRESSION_KINDS }

    /**
     * A name like [base] that nothing in [scope] already declares.
     *
     * Shadowing a live binding is the one way an extraction can silently break
     * the code around it, so the suffix loop is not cosmetic.
     */
    fun uniqueName(base: String, scope: PsiElement): String {
        val taken = HashSet<String>()
        scope.accept(object : com.intellij.psi.PsiRecursiveElementWalkingVisitor() {
            override fun visitElement(element: PsiElement) {
                (element as? JuxNamedElement)?.name?.let { taken.add(it) }
                super.visitElement(element)
            }
        })
        if (base !in taken) return base
        var i = 1
        while ("$base$i" in taken) i++
        return "$base$i"
    }

    private fun normalize(text: String) = text.replace(WHITESPACE, "")

    private val WHITESPACE = Regex("\\s+")
    private val INT_LITERAL = Regex("^-?(\\d[\\d_]*|0x[0-9a-fA-F_]+|0b[01_]+)$")
    private val LONG_LITERAL = Regex("^-?\\d[\\d_]*[lL]$")
    private val FLOAT_LITERAL = Regex("^-?\\d[\\d_]*(\\.\\d[\\d_]*)?[fF]$")
    private val DOUBLE_LITERAL = Regex("^-?\\d[\\d_]*\\.\\d[\\d_]*([dD])?$")
    private val INCREMENTS = listOf("++", "--")

    /** Expression kinds an extraction may target. */
    val EXPRESSION_KINDS = setOf(
        E.LITERAL_EXPRESSION,
        E.REFERENCE_EXPRESSION,
        E.BINARY_EXPRESSION,
        E.UNARY_EXPRESSION,
        E.CONDITIONAL_EXPRESSION,
        E.RANGE_EXPRESSION,
        E.CALL_EXPRESSION,
        E.INDEX_EXPRESSION,
        E.FIELD_ACCESS_EXPRESSION,
        E.CAST_EXPRESSION,
        E.NEW_EXPRESSION,
        E.SWITCH_EXPRESSION,
        E.PARENTHESIZED_EXPRESSION,
    )
}
