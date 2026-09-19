package dev.jux.intellij.intentions

import com.intellij.codeInsight.intention.PsiElementBaseIntentionAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.intentions.JuxCodeShapes as S
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * Base of the Jux Alt+Enter intentions, the counterparts of Java's
 * `codeInsight.intention.impl` set.
 *
 * Each intention finds its target from the element at the caret
 * ([target]), and rewrites it ([apply]). Availability is exactly "a target
 * exists", so the menu never offers what cannot run. The family name is the
 * intention's fixed name; [text] may be refined per target (`Flip '<' to '>'`).
 */
abstract class JuxIntention(private val family: String) : PsiElementBaseIntentionAction() {

    override fun getFamilyName(): String = family

    override fun isAvailable(project: Project, editor: Editor?, element: PsiElement): Boolean {
        if (element.containingFile !is JuxFile) return false
        val target = target(element) ?: return false
        text = textFor(target)
        return true
    }

    override fun invoke(project: Project, editor: Editor?, element: PsiElement) {
        val target = target(element) ?: return
        apply(target, editor)
    }

    /** The element this intention rewrites, found from the leaf at the caret, or null when unavailable. */
    protected abstract fun target(element: PsiElement): PsiElement?

    /** The menu text for [target]; the family name unless refined. */
    protected open fun textFor(target: PsiElement): String = family

    /** Rewrite [target]. */
    protected abstract fun apply(target: PsiElement, editor: Editor?)

    companion object {
        /** The nearest ancestor of [e] (itself included) of one of [types]. */
        fun ancestor(e: PsiElement, vararg types: com.intellij.psi.tree.IElementType): PsiElement? {
            var cur: PsiElement? = e
            while (cur != null && cur !is JuxFile) {
                if (types.any { cur!!.elementType === it }) return cur
                cur = cur.parent
            }
            return null
        }

        /**
         * The `if` whose header the caret is on: its `if` keyword or its
         * condition. The branches are someone else's.
         */
        fun ifAtHeader(e: PsiElement): PsiElement? {
            val parent = e.parent
            if (e.elementType === T.IF_KW && parent?.elementType === E.IF_STATEMENT) return parent
            val ifStmt = ancestor(e, E.IF_STATEMENT) ?: return null
            val cond = S.ifCondition(ifStmt) ?: return null
            return ifStmt.takeIf { PsiTreeUtil.isAncestor(cond, e, false) }
        }

        /** True when [type] is a type the editor fully knows, with nothing unresolved inside. */
        fun isKnown(type: JuxType): Boolean = when (type) {
            JuxType.Unknown -> false
            is JuxType.Static -> false
            is JuxType.ClassType -> type.args.all { isKnown(it) }
            is JuxType.ArrayType -> isKnown(type.element)
            is JuxType.Nullable -> isKnown(type.inner)
            is JuxType.Primitive -> type.name != "void"
            is JuxType.TypeVar -> true
            is JuxType.TupleType -> type.elements.all { isKnown(it) }
        }

        /** A local's initializer: the expression after its `=`, or null. */
        fun initializer(local: PsiElement): PsiElement? = S.compositeAfter(local, T.EQ)

        /** A local's name leaf. */
        fun nameOf(local: PsiElement): PsiElement? =
            generateSequence(local.firstChild) { it.nextSibling }.firstOrNull { it.elementType === T.IDENTIFIER }

        /**
         * The type a declaration needs written out: its own type, or for `var`
         * the type the editor infers for [value], or null when unknown.
         */
        fun writtenType(local: PsiElement, value: PsiElement?): String? {
            local.node.findChildByType(E.TYPE_REFERENCE)?.let { return it.text }
            if (value == null) return null
            val type = JuxTypeEngine.typeOf(value)
            return if (isKnown(type)) type.presentable() else null
        }

        /**
         * A local's declaration text up to its name, with `var` replaced by
         * [type]: `final int ` for `final var x = 1`.
         */
        fun declarationHead(local: PsiElement, type: String): String? {
            val name = nameOf(local) ?: return null
            val start = local.textRange.startOffset
            val head = local.text.substring(0, name.textRange.startOffset - start)
            val varKw = S.token(local, T.VAR_KW) ?: return head
            val at = varKw.textRange.startOffset - start
            return head.substring(0, at) + type + head.substring(at + varKw.textLength)
        }
    }
}

/**
 * Invert 'if' condition: negate the condition and swap the branches, so
 * `if (a == b) { x } else { y }` reads `if (a != b) { y } else { x }`. An `if`
 * without `else` gets an empty then-branch, as Java's does.
 */
class JuxInvertIfConditionIntention : JuxIntention("Invert 'if' condition") {
    override fun target(element: PsiElement): PsiElement? =
        ifAtHeader(element)?.takeIf { S.ifCondition(it) != null && S.ifThen(it) != null }

    override fun apply(target: PsiElement, editor: Editor?) {
        val cond = S.ifCondition(target) ?: return
        val then = S.ifThen(target) ?: return
        val otherwise = S.ifElse(target)
        val newThen = if (otherwise == null) "{\n}" else S.asBlockText(otherwise)
        S.replace(target, "if (${S.negate(cond)}) $newThen else ${S.asBlockText(then)}")
    }
}

/**
 * Flip binary operands: `a == b` to `b == a`, and `a < b` to `b > a`, with
 * the caret on the operator. Offered for the comparisons and the operators
 * whose operands commute.
 */
class JuxFlipBinaryIntention : JuxIntention("Flip binary expression") {
    private val mirrored = mapOf(
        T.EQ_EQ to "==", T.NOT_EQ to "!=", T.STRICT_EQ to "===", T.STRICT_NOT_EQ to "!==",
        T.LT to ">", T.GT to "<", T.LE to ">=", T.GE to "<=",
        T.AND_AND to "&&", T.OR_OR to "||", T.STAR to "*", T.AMP to "&", T.PIPE to "|", T.CARET to "^",
    )

    override fun target(element: PsiElement): PsiElement? {
        val binary = element.parent?.takeIf { it.elementType === E.BINARY_EXPRESSION } ?: return null
        if (S.operator(binary) != element || element.elementType !in mirrored.keys) return null
        return binary.takeIf { S.left(it) != null && S.right(it) != null && S.left(it) != S.right(it) }
    }

    override fun textFor(target: PsiElement): String {
        val op = S.operator(target) ?: return familyName
        val to = mirrored[op.elementType]
        return if (to == op.text) "Flip '${op.text}'" else "Flip '${op.text}' to '$to'"
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val op = S.operator(target) ?: return
        val left = S.left(target) ?: return
        val right = S.right(target) ?: return
        val level = S.precedence(op.elementType)
        // The old left operand becomes the right one, where a same-level
        // operator of its own would now group differently: `a == b == c`.
        val leftOp = S.operator(left)?.elementType
        val newRight = if (left.elementType === E.BINARY_EXPRESSION && S.precedence(leftOp) <= level &&
            !(leftOp === op.elementType && op.elementType in ASSOCIATIVE)
        ) "(${left.text})" else left.text
        S.replace(target, "${right.text} ${mirrored[op.elementType]} $newRight")
    }

    private companion object {
        val ASSOCIATIVE = setOf(T.AND_AND, T.OR_OR, T.STAR, T.AMP, T.PIPE, T.CARET)
    }
}

/**
 * Split declaration and assignment: `int x = 5;` becomes `int x;` and
 * `x = 5;`. A `var` declaration needs its type written, so it is offered only
 * when the editor knows the initializer's type.
 */
class JuxSplitDeclarationIntention : JuxIntention("Split into declaration and assignment") {
    override fun target(element: PsiElement): PsiElement? {
        val local = ancestor(element, E.LOCAL_VARIABLE) ?: return null
        if (local.parent?.elementType !== E.CODE_BLOCK) return null
        val init = initializer(local) ?: return null
        // The caret is on the declaration, not deep inside what it holds.
        if (PsiTreeUtil.isAncestor(init, element, false)) return null
        return local.takeIf { writtenType(it, init) != null }
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val init = initializer(target) ?: return
        val name = nameOf(target)?.text ?: return
        val head = declarationHead(target, writtenType(target, init) ?: return) ?: return
        S.replace(target, "${head.trimEnd()} $name;\n$name = ${init.text};")
    }
}

/**
 * Join declaration and assignment: `int x;` followed by `x = 5;` becomes
 * `int x = 5;`. Offered on either statement, when nothing but whitespace
 * separates them and the value does not read the variable itself.
 */
class JuxJoinDeclarationIntention : JuxIntention("Join declaration and assignment") {
    override fun target(element: PsiElement): PsiElement? {
        val local = ancestor(element, E.LOCAL_VARIABLE)
            ?: ancestor(element, E.EXPRESSION_STATEMENT)?.let { previousStatement(it) }
            ?: return null
        return local.takeIf { assignmentFor(it) != null }
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val statement = assignmentFor(target) ?: return
        val value = S.right(S.compositeChildren(statement).first()) ?: return
        val decl = target.text.trimEnd().removeSuffix(";").trimEnd()
        S.replace(target, TextRange(target.textRange.startOffset, statement.textRange.endOffset), "$decl = ${value.text};")
    }

    private fun previousStatement(statement: PsiElement): PsiElement? {
        var prev = statement.prevSibling
        while (prev is PsiWhiteSpace) prev = prev.prevSibling
        return prev?.takeIf { it.elementType === E.LOCAL_VARIABLE }
    }

    companion object {
        /**
         * The `x = value;` statement right after declaration [local], when the
         * two can be joined, or null.
         */
        fun assignmentFor(local: PsiElement): PsiElement? {
            if (local.elementType !== E.LOCAL_VARIABLE || local.parent?.elementType !== E.CODE_BLOCK) return null
            if (S.token(local, T.EQ) != null || S.token(local, T.VAR_KW) != null) return null
            val name = nameOf(local)?.text ?: return null
            var next = local.nextSibling
            while (next is PsiWhiteSpace) next = next.nextSibling
            if (next?.elementType !== E.EXPRESSION_STATEMENT) return null
            val assignment = S.compositeChildren(next).singleOrNull() ?: return null
            if (assignment.elementType !== E.ASSIGNMENT_EXPRESSION) return null
            if (S.operator(assignment)?.elementType !== T.EQ) return null
            val target = S.left(assignment) ?: return null
            val value = S.right(assignment) ?: return null
            if (target.elementType !== E.REFERENCE_EXPRESSION || target.text != name) return null
            val readsItself = PsiTreeUtil.collectElements(value) {
                it.elementType === E.REFERENCE_EXPRESSION && it.text == name
            }.isNotEmpty()
            return next.takeIf { !readsItself }
        }
    }
}

/**
 * The body a brace intention works on, found from the caret on a statement's
 * header: the keyword, or the condition. On `else` it is the else-branch.
 */
private fun braceTarget(element: PsiElement): Pair<PsiElement, PsiElement>? {
    val parent = element.parent ?: return null
    if (element.elementType === T.ELSE_KW && parent.elementType === E.IF_STATEMENT) {
        val body = S.ifElse(parent) ?: return null
        return (parent to body).takeIf { body.elementType !== E.IF_STATEMENT }
    }
    val owner = JuxIntention.ancestor(element, *S.BODY_OWNERS.types) ?: return null
    val body = (if (owner.elementType === E.IF_STATEMENT) S.ifThen(owner) else S.loopBody(owner)) ?: return null
    // The caret must be on the header, never inside the body itself.
    if (PsiTreeUtil.isAncestor(body, element, false)) return null
    return owner to body
}

/** The name of the construct a brace intention names in its text: `'else'` for an else-branch. */
private fun braceOwnerName(owner: PsiElement, body: PsiElement): String =
    if (owner.elementType === E.IF_STATEMENT && S.ifElse(owner) == body) "else" else S.keyword(owner)

/** Add braces: `if (c) x();` becomes `if (c) { x(); }`, for every statement with a body. */
class JuxAddBracesIntention : JuxIntention("Add braces to statement") {
    override fun target(element: PsiElement): PsiElement? =
        braceTarget(element)?.second?.takeIf { it.elementType !== E.CODE_BLOCK }

    override fun textFor(target: PsiElement): String =
        "Add braces to '${braceOwnerName(target.parent, target)}' statement"

    override fun apply(target: PsiElement, editor: Editor?) {
        S.replace(target, "{\n${target.text}\n}")
    }
}

/**
 * Remove braces: `if (c) { x(); }` becomes `if (c) x();`, when the block holds
 * one statement that can stand alone. Never where the unbraced statement
 * would capture a following `else`.
 */
class JuxRemoveBracesIntention : JuxIntention("Remove braces from statement") {
    override fun target(element: PsiElement): PsiElement? {
        val (owner, body) = braceTarget(element) ?: return null
        if (body.elementType !== E.CODE_BLOCK) return null
        val single = S.singleStatement(body) ?: return null
        if (single.elementType === E.LOCAL_VARIABLE || single.elementType === E.CODE_BLOCK) return null
        // `if (a) { if (b) x(); } else y();`: unbraced, the else would move.
        if (owner.elementType === E.IF_STATEMENT && S.ifThen(owner) == body && S.ifElse(owner) != null &&
            single.elementType === E.IF_STATEMENT && S.ifElse(single) == null
        ) return null
        return body
    }

    override fun textFor(target: PsiElement): String =
        "Remove braces from '${braceOwnerName(target.parent, target)}' statement"

    override fun apply(target: PsiElement, editor: Editor?) {
        val single = S.singleStatement(target) ?: return
        S.replace(target, single.text)
    }
}

/**
 * Replace `+` with string interpolation: `"Hello, " + name + "!"` becomes
 * `$"Hello, ${name}!"`. Jux formats strings by interpolation (there is no
 * `String.format` or builder to target), so this is the only rewrite.
 */
class JuxConcatToInterpolationIntention : JuxIntention("Replace '+' with string interpolation") {
    override fun target(element: PsiElement): PsiElement? {
        var top = generateSequence(element) { it.parent }.takeWhile { it !is JuxFile }
            .firstOrNull { S.isBinary(it, T.PLUS) } ?: return null
        while (S.isBinary(top.parent, T.PLUS)) top = top.parent
        val operands = operands(top)
        val firstString = operands.indexOfFirst { isStringLiteral(it) }
        // A string must arrive before two non-strings meet, or `1 + 2 + "a"`
        // would add first and read "3a".
        if (firstString !in 0..1) return null
        if (operands.any { it.text.contains('\n') || (!isStringLiteral(it) && (it.text.contains('{') || it.text.contains('}'))) }) return null
        return top
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val body = StringBuilder()
        for (operand in operands(target)) {
            val token = operand.firstChild
            when {
                token?.elementType === T.STRING_LITERAL -> body.append(escapeDollars(token.text.substring(1, token.textLength - 1)))
                token?.elementType === T.INTERP_STRING_LITERAL -> body.append(token.text.substring(2, token.textLength - 1))
                else -> body.append("\${").append(S.unparenthesized(operand).text).append('}')
            }
        }
        S.replace(target, "\$\"$body\"")
    }

    private fun operands(e: PsiElement): List<PsiElement> =
        if (S.isBinary(e, T.PLUS)) operands(S.left(e)!!) + listOfNotNull(S.right(e)) else listOf(e)

    /** A plain one-line string literal, interpolated or not; raw strings are left alone. */
    private fun isStringLiteral(e: PsiElement): Boolean {
        if (e.elementType !== E.LITERAL_EXPRESSION) return false
        val token = e.firstChild ?: return false
        return (token.elementType === T.STRING_LITERAL || token.elementType === T.INTERP_STRING_LITERAL) &&
            !token.text.startsWith("\"\"\"") && !token.text.startsWith("\$\"\"\"")
    }

    /** A literal `$` in a plain string is text; in an interpolated one it must be escaped. */
    private fun escapeDollars(s: String): String {
        val out = StringBuilder()
        var i = 0
        while (i < s.length) {
            val c = s[i]
            if (c == '\\' && i + 1 < s.length) {
                out.append(c).append(s[i + 1])
                i += 2
                continue
            }
            if (c == '$') out.append('\\')
            out.append(c)
            i++
        }
        return out.toString()
    }
}

/**
 * Replace string interpolation with `+`: `$"Hello, ${name}!"` becomes
 * `"Hello, " + name + "!"`. The reverse of [JuxConcatToInterpolationIntention].
 */
class JuxInterpolationToConcatIntention : JuxIntention("Replace string interpolation with '+'") {
    override fun target(element: PsiElement): PsiElement? {
        if (element.elementType !== T.INTERP_STRING_LITERAL || element.text.startsWith("\$\"\"\"")) return null
        return element.parent?.takeIf { it.elementType === E.LITERAL_EXPRESSION && parts(element.text) != null }
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val parts = parts(target.text) ?: return
        val pieces = ArrayList<String>()
        for ((isExpr, text) in parts) {
            if (!isExpr) {
                if (text.isNotEmpty()) pieces.add("\"$text\"")
                continue
            }
            pieces.add(if (SIMPLE.matches(text.trim())) text.trim() else "(${text.trim()})")
        }
        // Two values meeting first would add, not concatenate.
        val firstIsText = pieces.firstOrNull()?.startsWith("\"") == true
        val secondIsText = pieces.getOrNull(1)?.startsWith("\"") == true
        if (!firstIsText && !secondIsText) pieces.add(0, "\"\"")
        S.replace(target, pieces.joinToString(" + "))
    }

    companion object {
        /** An operand that needs no parentheses beside `+`: a name, member chain, call or literal. */
        private val SIMPLE = Regex("""[\w.]+(\([^()]*\))?(\.\w+(\([^()]*\))?)*|"[^"]*"|'[^']*'""")

        /**
         * The chunks of an interpolated literal `$"..."`: (false, text) for
         * literal text as written (escapes kept), (true, expression) for
         * `${expr}` and `$name`. Null when a hole is unterminated.
         */
        fun parts(literal: String): List<Pair<Boolean, String>>? {
            val body = literal.removePrefix("\$\"").removeSuffix("\"")
            val out = ArrayList<Pair<Boolean, String>>()
            val text = StringBuilder()
            var i = 0
            fun flush() {
                out.add(false to text.toString())
                text.setLength(0)
            }
            while (i < body.length) {
                val c = body[i]
                if (c == '\\' && i + 1 < body.length) {
                    // `\$` is a literal dollar; in a plain string `$` needs no escape.
                    if (body[i + 1] == '$') text.append('$') else text.append(c).append(body[i + 1])
                    i += 2
                    continue
                }
                if (c == '$' && i + 1 < body.length && body[i + 1] == '{') {
                    var depth = 1
                    var j = i + 2
                    while (j < body.length && depth > 0) {
                        if (body[j] == '{') depth++ else if (body[j] == '}') depth--
                        j++
                    }
                    if (depth != 0) return null
                    flush()
                    out.add(true to body.substring(i + 2, j - 1))
                    i = j
                    continue
                }
                if (c == '$' && i + 1 < body.length && (body[i + 1].isLetter() || body[i + 1] == '_')) {
                    var j = i + 1
                    while (j < body.length && (body[j].isLetterOrDigit() || body[j] == '_')) j++
                    flush()
                    out.add(true to body.substring(i + 1, j))
                    i = j
                    continue
                }
                text.append(c)
                i++
            }
            flush()
            return out
        }
    }
}

/**
 * Replace `?:` with `if else`: `return c ? a : b;`, `x = c ? a : b;` and
 * `var x = c ? a : b;` become the `if`/`else` that does the same.
 */
class JuxTernaryToIfIntention : JuxIntention("Replace '?:' with 'if else'") {
    override fun target(element: PsiElement): PsiElement? {
        val conditional = ancestor(element, E.CONDITIONAL_EXPRESSION) ?: return null
        if (S.compositeChildren(conditional).size != 3) return null
        val parent = conditional.parent ?: return null
        return when (parent.elementType) {
            E.RETURN_STATEMENT -> conditional
            E.ASSIGNMENT_EXPRESSION -> conditional.takeIf {
                S.right(parent) == it && parent.parent?.elementType === E.EXPRESSION_STATEMENT
            }
            E.LOCAL_VARIABLE -> conditional.takeIf {
                initializer(parent) == it && parent.parent?.elementType === E.CODE_BLOCK && writtenType(parent, it) != null
            }
            else -> null
        }
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val (cond, whenTrue, whenFalse) = S.compositeChildren(target)
        val parent = target.parent
        fun branches(stmt: (String) -> String) =
            "if (${cond.text}) {\n${stmt(whenTrue.text)}\n} else {\n${stmt(whenFalse.text)}\n}"
        when (parent.elementType) {
            E.RETURN_STATEMENT -> S.replace(parent, branches { "return $it;" })
            E.ASSIGNMENT_EXPRESSION -> {
                val lhs = S.left(parent)?.text ?: return
                val op = S.operator(parent)?.text ?: return
                S.replace(parent.parent, branches { "$lhs $op $it;" })
            }
            E.LOCAL_VARIABLE -> {
                val name = nameOf(parent)?.text ?: return
                val head = declarationHead(parent, writtenType(parent, target) ?: return) ?: return
                S.replace(parent, "${head.trimEnd()} $name;\n" + branches { "$name = $it;" })
            }
        }
    }
}

/**
 * Replace `if else` with `?:`: an `if`/`else` whose branches each return a
 * value, or each assign the same target with the same operator, becomes one
 * statement.
 */
class JuxIfToTernaryIntention : JuxIntention("Replace 'if else' with '?:'") {
    override fun target(element: PsiElement): PsiElement? {
        val ifStmt = (if (element.elementType === T.ELSE_KW) element.parent else ifAtHeader(element)) ?: return null
        if (ifStmt.elementType !== E.IF_STATEMENT) return null
        return ifStmt.takeIf { shape(it) != null }
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val (prefix, a, b) = shape(target) ?: return
        val cond = S.ifCondition(target) ?: return
        val arm = { e: PsiElement -> S.operandText(e, S.precedence(T.QUESTION_COLON) + 1) }
        S.replace(target, "$prefix${arm(cond)} ? ${arm(a)} : ${arm(b)};")
    }

    /** `(statement prefix, then value, else value)`, or null when the branches do not match. */
    private fun shape(ifStmt: PsiElement): Triple<String, PsiElement, PsiElement>? {
        val elseBody = S.ifElse(ifStmt)?.takeIf { it.elementType !== E.IF_STATEMENT } ?: return null
        val t = S.ifThen(ifStmt)?.let { S.singleStatement(it) } ?: return null
        val f = S.singleStatement(elseBody) ?: return null
        if (t.elementType === E.RETURN_STATEMENT && f.elementType === E.RETURN_STATEMENT) {
            val a = S.compositeChildren(t).singleOrNull() ?: return null
            val b = S.compositeChildren(f).singleOrNull() ?: return null
            return Triple("return ", a, b)
        }
        if (t.elementType === E.EXPRESSION_STATEMENT && f.elementType === E.EXPRESSION_STATEMENT) {
            val ta = S.compositeChildren(t).singleOrNull()?.takeIf { it.elementType === E.ASSIGNMENT_EXPRESSION } ?: return null
            val fa = S.compositeChildren(f).singleOrNull()?.takeIf { it.elementType === E.ASSIGNMENT_EXPRESSION } ?: return null
            val op = S.operator(ta)?.text ?: return null
            if (op != S.operator(fa)?.text || S.left(ta)?.text != S.left(fa)?.text) return null
            return Triple("${S.left(ta)!!.text} $op ", S.right(ta) ?: return null, S.right(fa) ?: return null)
        }
        return null
    }
}

/**
 * Merge nested 'if's: `if (a) { if (b) { x } }`, neither with an `else`,
 * becomes `if (a && b) { x }`.
 */
class JuxMergeNestedIfsIntention : JuxIntention("Merge nested 'if's") {
    override fun target(element: PsiElement): PsiElement? {
        val outer = ifAtHeader(element) ?: return null
        return outer.takeIf { inner(it) != null }
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val inner = inner(target) ?: return
        val and = S.precedence(T.AND_AND)
        val a = S.operandText(S.ifCondition(target) ?: return, and)
        val b = S.operandText(S.ifCondition(inner) ?: return, and)
        S.replace(target, "if ($a && $b) ${S.ifThen(inner)?.text ?: return}")
    }

    private fun inner(outer: PsiElement): PsiElement? {
        if (S.ifElse(outer) != null) return null
        val inner = S.ifThen(outer)?.let { S.singleStatement(it) } ?: return null
        return inner.takeIf { it.elementType === E.IF_STATEMENT && S.ifElse(it) == null && S.ifThen(it) != null }
    }
}

/**
 * Split into 2 'if' statements: with the caret on a `&&` of the condition,
 * `if (a && b) { x }` becomes `if (a) { if (b) { x } }`. Only for an `if`
 * without `else`, whose meaning a split cannot change.
 */
class JuxSplitAndIntoIfsIntention : JuxIntention("Split into 2 'if' statements") {
    override fun target(element: PsiElement): PsiElement? {
        if (element.elementType !== T.AND_AND) return null
        val binary = element.parent?.takeIf { S.isBinary(it, T.AND_AND) } ?: return null
        var top = binary
        while (S.isBinary(top.parent, T.AND_AND)) top = top.parent
        val ifStmt = top.parent?.takeIf { it.elementType === E.IF_STATEMENT && S.ifCondition(it) == top } ?: return null
        return binary.takeIf { S.ifElse(ifStmt) == null && S.ifThen(ifStmt) != null }
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        var top = target
        while (S.isBinary(top.parent, T.AND_AND)) top = top.parent
        val ifStmt = top.parent
        val left = S.left(target)?.text ?: return
        // Everything right of the chosen `&&`: its right operand, then every
        // operand the enclosing `&&`s add after it.
        val rights = ArrayList<String>()
        var node: PsiElement = target
        rights.add(S.right(node)?.text ?: return)
        while (node != top) {
            node = node.parent
            rights.add(S.right(node)?.text ?: return)
        }
        val then = S.ifThen(ifStmt)?.text ?: return
        S.replace(ifStmt, "if ($left) {\nif (${rights.joinToString(" && ")}) $then\n}")
    }
}

/**
 * Replace 'var' with explicit type: `var names = new Vec<String>();` becomes
 * `Vec<String> names = new Vec<String>();`, when the editor knows the type.
 */
class JuxVarToExplicitTypeIntention : JuxIntention("Replace 'var' with explicit type") {
    override fun target(element: PsiElement): PsiElement? {
        val local = ancestor(element, E.LOCAL_VARIABLE) ?: return null
        val init = initializer(local) ?: return null
        if (PsiTreeUtil.isAncestor(init, element, false)) return null
        if (S.token(local, T.VAR_KW) == null) return null
        return local.takeIf { writtenType(it, init) != null }
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val type = writtenType(target, initializer(target) ?: return) ?: return
        val varKw = S.token(target, T.VAR_KW) ?: return
        S.replace(varKw, type)
    }
}

/**
 * Replace explicit type with 'var': `Vec<String> names = new Vec<String>();`
 * becomes `var names = new Vec<String>();`. Only where the initializer has
 * exactly the declared type, so `double d = 1;` keeps its `double`.
 */
class JuxExplicitTypeToVarIntention : JuxIntention("Replace explicit type with 'var'") {
    override fun target(element: PsiElement): PsiElement? {
        val local = ancestor(element, E.LOCAL_VARIABLE) ?: return null
        val init = initializer(local) ?: return null
        if (PsiTreeUtil.isAncestor(init, element, false)) return null
        val declared = local.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return null
        val inferred = JuxTypeEngine.typeOf(init)
        if (!isKnown(inferred)) return null
        return local.takeIf { normalize(inferred.presentable()) == normalize(declared.text) }
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val declared = target.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return
        S.replace(declared, "var")
    }

    private fun normalize(s: String) = s.replace(Regex("\\s+"), "")
}

/**
 * Merge 'else if': `else { if (c) { ... } }` becomes `else if (c) { ... }`,
 * when the else-block holds that `if` alone.
 */
class JuxMergeElseIfIntention : JuxIntention("Merge 'else if'") {
    override fun target(element: PsiElement): PsiElement? {
        if (element.elementType !== T.ELSE_KW) return null
        val ifStmt = element.parent?.takeIf { it.elementType === E.IF_STATEMENT } ?: return null
        val block = S.ifElse(ifStmt)?.takeIf { it.elementType === E.CODE_BLOCK } ?: return null
        val single = S.singleStatement(block) ?: return null
        return block.takeIf { single.elementType === E.IF_STATEMENT }
    }

    override fun apply(target: PsiElement, editor: Editor?) {
        val single = S.singleStatement(target) ?: return
        S.replace(target, single.text)
    }
}
