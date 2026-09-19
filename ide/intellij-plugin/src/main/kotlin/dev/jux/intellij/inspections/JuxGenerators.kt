package dev.jux.intellij.inspections

import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * Facts about generators (Missing-defs §M.2) the editor needs in more than one
 * place: which function a `yield` belongs to, whether a function is a
 * generator, and whether its declared return type is the one a generator must
 * have (`Iterator<T>`, or `Stream<T>` when `async`).
 *
 * A generator is a named function or method whose own body contains `yield`.
 * A `yield` inside a lambda, a constructor, a nested class or a switch
 * expression's arm belongs to none (the compiler's E0990).
 */
object JuxGenerators {

    /** Node types that start a new function body: a `yield` never crosses one. */
    private val FUNCTION_BOUNDARIES = setOf(
        E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION,
        E.LAMBDA_EXPRESSION, E.PROPERTY_DECLARATION, E.FIELD_DECLARATION,
        E.CLASS_DECLARATION, E.INTERFACE_DECLARATION, E.ENUM_DECLARATION,
        E.RECORD_DECLARATION, E.STRUCT_DECLARATION,
    )

    /**
     * The nearest enclosing function-like node of [e]: a method, constructor,
     * operator, lambda, property or field initializer, or a type (for code in
     * an initializer block). Null at the top level of a file.
     */
    fun owner(e: PsiElement): PsiElement? {
        var p = e.parent
        while (p != null && p !is PsiFile) {
            if (p.elementType in FUNCTION_BOUNDARIES) return p
            p = p.parent
        }
        return null
    }

    /** The `yield` keywords that belong to [function] itself (not to a nested lambda or class). */
    fun ownYields(function: PsiElement): List<PsiElement> {
        val body = function.node.findChildByType(E.CODE_BLOCK)?.psi ?: return emptyList()
        return PsiTreeUtil.collectElements(body) { it.elementType === T.YIELD_KW && owner(it) === function }.toList()
    }

    /** Whether [function] is a generator: a named function or method with a `yield` of its own. */
    fun isGenerator(function: PsiElement): Boolean =
        function.elementType === E.METHOD_DECLARATION && ownYields(function).isNotEmpty()

    /** Whether [function] is declared `async` (its generator form returns `Stream<T>`). */
    fun isAsync(function: PsiElement): Boolean = JuxHierarchy.hasModifier(function, "async")

    /** The bare name of the declared return type: `Iterator` of `Iterator<int>`, or null. */
    fun returnTypeName(function: PsiElement): String? {
        val ref = function.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return null
        return JuxHierarchy.bareTypeName(ref)
    }

    /** The name the return type of [function] must have as a generator: `Stream` if async, else `Iterator`. */
    fun expectedReturnName(function: PsiElement): String = if (isAsync(function)) "Stream" else "Iterator"

    /** Whether [function] is declared to return a sequence a generator may produce. */
    fun returnsSequence(function: PsiElement): Boolean = returnTypeName(function) == expectedReturnName(function)

    /**
     * Whether a `yield` statement written at [at] would be legal: inside a
     * named function (or method) declared to return `Iterator<T>` / `Stream<T>`,
     * outside a switch expression's arm and an `unsafe { }` block.
     */
    fun yieldAllowedAt(at: PsiElement): Boolean {
        val fn = owner(at) ?: return false
        if (fn.elementType !== E.METHOD_DECLARATION || !returnsSequence(fn)) return false
        return !insideBefore(at, fn, E.SWITCH_EXPRESSION) && !insideBefore(at, fn, E.UNSAFE_STATEMENT)
    }

    /** Whether [e] sits inside a node of [type] that is itself inside [stop]. */
    fun insideBefore(e: PsiElement, stop: PsiElement, type: com.intellij.psi.tree.IElementType): Boolean {
        var p = e.parent
        while (p != null && p !== stop) {
            if (p.elementType === type) return true
            p = p.parent
        }
        return false
    }
}
