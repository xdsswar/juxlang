package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * The layout and ABI rules of Layout-ABI §L.1.4, §L.6.3, §L.7.4 and the
 * operator coherence rules of Runtime §R.3, checked as you type. Each check
 * covers only the part of the compiler's rule the editor can decide from the
 * source alone; the rest (a type's real size or alignment, which module owns
 * a type) is left to `juxc`, whose diagnostic arrives through the language
 * server.
 *
 * - E0519: `@align(N)` whose argument is not an integer literal, or not a
 *   power of two.
 * - E0520: `@align` on a field, an enum or an interface.
 * - E0521: `array as T*` from a temporary array, from a nested array, or to a
 *   pointer to a pointer.
 * - E0522: `transmute` without exactly two type arguments, or between two
 *   primitive types whose sizes differ.
 * - E0951: the same free operator declared twice in one file.
 * - E0950: a free operator on types none of which this program declares (only
 *   decided when every operand and the result are primitives or `String`).
 */
class JuxAbiRulesInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                when (element.elementType) {
                    E.ANNOTATION -> checkAlign(element, holder)
                    E.CAST_EXPRESSION -> checkArrayToPointer(element, holder)
                    E.CALL_EXPRESSION -> checkTransmute(element, holder)
                    E.OPERATOR_DECLARATION -> if (element.parent is JuxFile) checkFreeOperator(element, holder)
                }
            }
        }

    // ---------------------------------------------------------------- @align

    private fun checkAlign(ann: PsiElement, holder: ProblemsHolder) {
        val name = ann.text.removePrefix("@").substringBefore('(').trim()
        if (!name.equals("align", ignoreCase = true)) return
        val owner = ann.parent ?: return
        val placement = when (owner.elementType) {
            E.FIELD_DECLARATION -> "a field"
            E.ENUM_DECLARATION -> "an enum"
            E.INTERFACE_DECLARATION -> "an interface"
            else -> null
        }
        if (placement != null) {
            holder.registerProblem(
                ann,
                "`@align` cannot be put on $placement (E0520)",
                ProblemHighlightType.GENERIC_ERROR,
            )
            return
        }
        val arg = ann.text.substringAfter('(', "").substringBeforeLast(')', "").trim()
        if (arg.isEmpty()) return
        val n = parseIntLiteral(arg)
        if (n == null) {
            holder.registerProblem(
                ann,
                "The argument of `@align` must be an integer literal, such as `@align(64)` (E0519)",
                ProblemHighlightType.GENERIC_ERROR,
            )
        } else if (n <= 0 || (n and (n - 1)) != 0L) {
            holder.registerProblem(
                ann,
                "`@align($arg)` cannot hold: $n is not a power of two (E0519)",
                ProblemHighlightType.GENERIC_ERROR,
            )
        }
    }

    /** A decimal or `0x` literal with optional `_` separators, or null. */
    private fun parseIntLiteral(text: String): Long? {
        val t = text.replace("_", "")
        return when {
            t.startsWith("0x") || t.startsWith("0X") -> t.drop(2).toLongOrNull(16)
            else -> t.toLongOrNull()
        }
    }

    // ------------------------------------------------------ array as T*

    private fun checkArrayToPointer(cast: PsiElement, holder: ProblemsHolder) {
        val target = cast.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return
        val targetText = target.text.replace(" ", "")
        if (!targetText.endsWith("*")) return
        var source = JuxTypeEngine.firstExpressionChild(cast) ?: return
        while (source.elementType === E.PARENTHESIZED_EXPRESSION) {
            source = JuxTypeEngine.firstExpressionChild(source) ?: return
        }
        val reason = when {
            source.elementType === E.NEW_EXPRESSION && source.text.contains('[') ->
                "the array must be a named one (a local, a parameter or a field): a temporary array " +
                    "is freed at the end of the statement, and the pointer would dangle"
            else -> {
                val type = JuxTypeEngine.stripNullable(JuxTypeEngine.typeOf(source)) as? JuxType.ArrayType ?: return
                when {
                    type.element is JuxType.ArrayType ->
                        "only a one-dimensional array converts to a pointer: the elements of a nested " +
                            "array are handles to other arrays"
                    targetText.endsWith("**") ->
                        "an array converts to a pointer to its elements (`T*`), not to a pointer to a pointer"
                    else -> return
                }
            }
        }
        holder.registerProblem(
            cast,
            "Cannot convert this array to a pointer: $reason (E0521)",
            ProblemHighlightType.GENERIC_ERROR,
        )
    }

    // -------------------------------------------------------- transmute

    private fun checkTransmute(call: PsiElement, holder: ProblemsHolder) {
        val callee = call.firstChild ?: return
        if (callee.elementType !== E.REFERENCE_EXPRESSION) return
        if (callee.text.substringBefore('<').trim() != "transmute") return
        // A user function named `transmute` shadows the built-in.
        if (JuxTypeEngine.resolveReferenceExpression(callee) != null) return
        val typeArgs = callee.node.findChildByType(E.TYPE_ARGUMENT_LIST)?.psi
            ?: call.node.findChildByType(E.TYPE_ARGUMENT_LIST)?.psi ?: return
        // The list's depth-1 segments, as written (`int*` keeps its `*`, which
        // the type-argument PSI leaves outside the name's TYPE_REFERENCE).
        val args = splitTopLevel(typeArgs.text.trim().removePrefix("<").removeSuffix(">"))
        if (args.size != 2) {
            holder.registerProblem(
                typeArgs,
                "`transmute` takes exactly two type arguments, the type it reads and the type it gives: " +
                    "`transmute<A, B>(value)` (E0522)",
                ProblemHighlightType.GENERIC_ERROR,
            )
            return
        }
        val (a, b) = args
        val sa = sizeOf(a) ?: return
        val sb = sizeOf(b) ?: return
        if (sa == sb) return
        holder.registerProblem(
            typeArgs,
            "Invalid `transmute`: the two types must be the same size: `$a` is ${describe(sa)} " +
                "and `$b` is ${describe(sb)} (E0522)",
            ProblemHighlightType.GENERIC_ERROR,
        )
    }

    /** [text] split at its depth-0 commas, each piece with whitespace removed; empty pieces dropped. */
    private fun splitTopLevel(text: String): List<String> {
        val out = ArrayList<String>()
        var depth = 0
        val cur = StringBuilder()
        for (c in text) {
            when (c) {
                '<', '(', '[' -> depth++
                '>', ')', ']' -> depth--
            }
            if (c == ',' && depth == 0) {
                out.add(cur.toString()); cur.clear()
            } else if (!c.isWhitespace()) cur.append(c)
        }
        out.add(cur.toString())
        return out.filter { it.isNotEmpty() }
    }

    /** Bytes for a fixed-size primitive; [WORD] for `int`, `uint` and pointers; null otherwise. */
    private fun sizeOf(type: String): Int? = when {
        type.endsWith("*") -> WORD
        else -> PRIMITIVE_SIZES[type]
    }

    private fun describe(size: Int): String =
        if (size == WORD) "one machine word (4 or 8 bytes by target)" else "$size bytes"

    // ---------------------------------------------------- free operators

    /**
     * A top-level operator: a repeat of an earlier one in the same file is
     * E0951 (reported on the repeat, as the compiler does), and one whose
     * operands and result are all types no program declares is E0950.
     */
    private fun checkFreeOperator(op: PsiElement, holder: ProblemsHolder) {
        val signature = signatureOf(op) ?: return
        var prev = op.prevSibling
        while (prev != null) {
            if (prev.elementType === E.OPERATOR_DECLARATION && signatureOf(prev)?.first == signature.first) {
                holder.registerProblem(
                    op,
                    "`${signature.first}` is declared more than once: a call could not tell which one runs; keep one (E0951)",
                    ProblemHighlightType.GENERIC_ERROR,
                )
                return
            }
            prev = prev.prevSibling
        }
        val params = signature.second
        val result = dev.jux.intellij.resolve.JuxHierarchy.returnTypeText(op)?.replace(" ", "") ?: return
        if (params.isNotEmpty() && params.all { it in FOREIGN_OWNED } && result in FOREIGN_OWNED) {
            holder.registerProblem(
                op,
                "Orphan operator: `${signature.first}` is declared here, but none of its types is declared by " +
                    "this program, so it would redefine `${operatorSymbol(op)}` on types it does not own (E0950)",
                ProblemHighlightType.GENERIC_ERROR,
            )
        }
    }

    /** `operator*(double, Vec3)` and its parameter types, or null when unreadable. */
    private fun signatureOf(op: PsiElement): Pair<String, List<String>>? {
        val symbol = operatorSymbol(op) ?: return null
        val params = op.node.findChildByType(E.PARAMETER_LIST)?.psi
            ?.children?.filter { it.elementType === E.PARAMETER }
            ?.map { it.node.findChildByType(E.TYPE_REFERENCE)?.text?.replace(" ", "") ?: "?" }
            ?: return null
        return "operator$symbol(${params.joinToString(", ")})" to params
    }

    /** The operator's symbol text (`+`, `*`, `==`), read after the `operator` keyword. */
    private fun operatorSymbol(op: PsiElement): String? {
        val text = op.text
        val at = Regex("""\boperator\s*""").find(text) ?: return null
        val rest = text.substring(at.range.last + 1)
        return rest.substringBefore('(').trim().takeIf { it.isNotEmpty() }
    }

    private companion object {
        /** Stands for "one machine word": pairs only with itself. */
        const val WORD = -1

        val PRIMITIVE_SIZES = mapOf(
            "byte" to 1, "ubyte" to 1, "i8" to 1, "u8" to 1, "bool" to 1,
            "short" to 2, "ushort" to 2, "i16" to 2, "u16" to 2,
            "i32" to 4, "u32" to 4, "float" to 4, "f32" to 4,
            "long" to 8, "ulong" to 8, "i64" to 8, "u64" to 8, "double" to 8, "f64" to 8,
            "int" to WORD, "uint" to WORD,
        )

        /** Types no Jux program declares: primitives and `String`. */
        val FOREIGN_OWNED: Set<String> = PRIMITIVE_SIZES.keys + setOf("String", "void")
    }
}
