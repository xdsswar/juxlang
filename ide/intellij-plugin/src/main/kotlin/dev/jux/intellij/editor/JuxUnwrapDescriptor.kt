package dev.jux.intellij.editor

import com.intellij.codeInsight.unwrap.AbstractUnwrapper
import com.intellij.codeInsight.unwrap.UnwrapDescriptorBase
import com.intellij.codeInsight.unwrap.Unwrapper
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.intentions.JuxCodeShapes as S
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Unwrap/Remove (Ctrl+Shift+Delete) for Jux, the counterpart of Java's
 * `JavaUnwrapDescriptor`: with the caret inside a construct, offer to keep
 * its body and drop the construct around it (`if`, `else`, the loops, `try`,
 * a bare block, a lambda body), or to remove a part of it (`else`, `catch`,
 * `finally`).
 *
 * The platform walks from the caret outwards and lists every unwrapper that
 * applies to each enclosing element, innermost first.
 */
class JuxUnwrapDescriptor : UnwrapDescriptorBase() {
    override fun createUnwrappers(): Array<Unwrapper> = arrayOf(
        JuxIfUnwrapper(),
        JuxElseUnwrapper(),
        JuxElseRemover(),
        JuxLoopUnwrapper(),
        JuxTryUnwrapper(),
        JuxCatchRemover(),
        JuxFinallyRemover(),
        JuxBracesUnwrapper(),
        JuxLambdaUnwrapper(),
    )
}

/**
 * Shared shape of the Jux unwrappers: extract a body's statements in front of
 * the construct being unwrapped, then delete the construct.
 */
abstract class JuxUnwrapper(description: String) : AbstractUnwrapper<JuxUnwrapper.Context>(description) {
    override fun createContext(): Context = Context()

    /** What an unwrapper moves and deletes, recorded without editing while the popup only previews. */
    class Context : AbstractContext() {
        override fun isWhiteSpace(element: PsiElement): Boolean = element is PsiWhiteSpace

        /**
         * Move the statements of [body] in front of [from]: the contents of a
         * `{ }` block, or a single unbraced statement.
         */
        fun extractBody(body: PsiElement?, from: PsiElement) {
            if (body == null) return
            if (body.elementType === E.CODE_BLOCK) {
                val first = body.firstChild?.nextSibling ?: return
                val last = body.lastChild?.prevSibling ?: return
                if (first.elementType === T.RBRACE || body.firstChild.elementType !== T.LBRACE ||
                    body.lastChild.elementType !== T.RBRACE
                ) return
                extract(first, last, from)
            } else if (body.elementType !== E.EMPTY_STATEMENT) {
                extract(body, body, from)
            }
        }
    }
}

/** Unwrap 'if...': keep the then-branch, drop the condition and any `else`. */
class JuxIfUnwrapper : JuxUnwrapper("Unwrap 'if...'") {
    override fun isApplicableTo(e: PsiElement): Boolean = e.elementType === E.IF_STATEMENT && !S.isElseBranch(e)

    override fun doUnwrap(element: PsiElement, context: Context) {
        context.extractBody(S.ifThen(element), element)
        context.delete(element)
    }
}

/** Is [e] the `else` keyword, or the else-branch, of an `if` that has one? */
private fun elseOf(e: PsiElement): Pair<PsiElement, PsiElement>? {
    val parent = e.parent ?: return null
    if (parent.elementType !== E.IF_STATEMENT) return null
    val branch = S.ifElse(parent) ?: return null
    return if (e.elementType === T.ELSE_KW || e == branch) parent to branch else null
}

/**
 * Unwrap 'else...': keep the else-branch and drop the `if` it hangs off,
 * then-branch included. Like Java's, it replaces the whole chain from the top
 * `if` down.
 */
class JuxElseUnwrapper : JuxUnwrapper("Unwrap 'else...'") {
    override fun isApplicableTo(e: PsiElement): Boolean = elseOf(e) != null

    override fun collectElementsToIgnore(element: PsiElement, result: MutableSet<PsiElement>) {
        var parent = element.parent
        while (parent?.elementType === E.IF_STATEMENT) {
            result.add(parent)
            parent = parent.parent
        }
    }

    override fun doUnwrap(element: PsiElement, context: Context) {
        val (ifStmt, branch) = elseOf(element) ?: return
        var top = ifStmt
        while (S.isElseBranch(top)) top = top.parent
        context.extractBody(branch, top)
        context.delete(top)
    }
}

/** Remove 'else...': drop the else-branch and keep the `if` without it. */
class JuxElseRemover : JuxUnwrapper("Remove 'else...'") {
    override fun isApplicableTo(e: PsiElement): Boolean = elseOf(e) != null

    override fun collectElementsToIgnore(element: PsiElement, result: MutableSet<PsiElement>) {
        var parent = element.parent
        while (parent?.elementType === E.IF_STATEMENT) {
            result.add(parent)
            parent = parent.parent
        }
    }

    override fun doUnwrap(element: PsiElement, context: Context) {
        val (ifStmt, branch) = elseOf(element) ?: return
        if (!context.isEffective) return
        // `else if (c) x else y`: only this `if` goes, its own `else` moves up.
        val nested = if (branch.elementType === E.IF_STATEMENT) S.ifElse(branch) else null
        if (nested != null) {
            S.replace(branch, nested.text)
            return
        }
        val elseKw = S.token(ifStmt, T.ELSE_KW) ?: return
        val from = elseKw.prevSibling?.takeIf { it is PsiWhiteSpace } ?: elseKw
        ifStmt.deleteChildRange(from, branch)
    }
}

/** Unwrap 'while...', 'for...' and 'do...': keep the loop's body, run once, in its place. */
class JuxLoopUnwrapper : JuxUnwrapper("Unwrap loop") {
    override fun isApplicableTo(e: PsiElement): Boolean =
        e.elementType === E.WHILE_STATEMENT || e.elementType === E.FOR_STATEMENT ||
            e.elementType === E.FOR_EACH_STATEMENT || e.elementType === E.DO_WHILE_STATEMENT

    override fun getDescription(e: PsiElement): String = "Unwrap '${S.keyword(e)}...'"

    override fun doUnwrap(element: PsiElement, context: Context) {
        context.extractBody(S.loopBody(element), element)
        context.delete(element)
    }
}

/** Unwrap 'try...': keep the `try` body and drop every `catch` and `finally`. */
class JuxTryUnwrapper : JuxUnwrapper("Unwrap 'try...'") {
    override fun isApplicableTo(e: PsiElement): Boolean = e.elementType === E.TRY_STATEMENT

    override fun doUnwrap(element: PsiElement, context: Context) {
        context.extractBody(S.compositeAfter(element, T.TRY_KW), element)
        context.delete(element)
    }
}

/**
 * Remove 'catch...': drop one `catch` clause. The last handler of a `try`
 * without `finally` takes the `try` with it, keeping its body, since a bare
 * `try { }` is not a statement.
 */
class JuxCatchRemover : JuxUnwrapper("Remove 'catch...'") {
    override fun isApplicableTo(e: PsiElement): Boolean = e.elementType === E.CATCH_CLAUSE

    override fun doUnwrap(element: PsiElement, context: Context) {
        val tryStmt = element.parent ?: return
        val handlers = S.compositeChildren(tryStmt).filter {
            it.elementType === E.CATCH_CLAUSE || it.elementType === E.FINALLY_CLAUSE
        }
        if (handlers.size > 1) {
            context.delete(element)
            return
        }
        context.extractBody(S.compositeAfter(tryStmt, T.TRY_KW), tryStmt)
        context.delete(tryStmt)
    }
}

/**
 * Remove 'finally...': drop the `finally` clause. Without any `catch` left,
 * the `try` goes too and its body stays.
 */
class JuxFinallyRemover : JuxUnwrapper("Remove 'finally...'") {
    override fun isApplicableTo(e: PsiElement): Boolean = e.elementType === E.FINALLY_CLAUSE

    override fun doUnwrap(element: PsiElement, context: Context) {
        val tryStmt = element.parent ?: return
        if (S.compositeChildren(tryStmt).any { it.elementType === E.CATCH_CLAUSE }) {
            context.delete(element)
            return
        }
        context.extractBody(S.compositeAfter(tryStmt, T.TRY_KW), tryStmt)
        context.delete(tryStmt)
    }
}

/** Unwrap braces: a bare `{ ... }` block standing as a statement gives its statements to the enclosing block. */
class JuxBracesUnwrapper : JuxUnwrapper("Unwrap braces") {
    override fun isApplicableTo(e: PsiElement): Boolean =
        e.elementType === E.CODE_BLOCK && e.parent?.elementType === E.CODE_BLOCK

    override fun doUnwrap(element: PsiElement, context: Context) {
        context.extractBody(element, element)
        context.delete(element)
    }
}

/**
 * Unwrap lambda: `run(() -> { work(); })` as a statement becomes `work();`,
 * the body's statements in place of the statement that held the lambda.
 * Offered where Java offers it: a block-bodied lambda inside an expression
 * statement.
 */
class JuxLambdaUnwrapper : JuxUnwrapper("Unwrap lambda...") {
    override fun isApplicableTo(e: PsiElement): Boolean {
        if (e.elementType !== E.LAMBDA_EXPRESSION) return false
        if (S.compositeChildren(e).lastOrNull()?.elementType !== E.CODE_BLOCK) return false
        return statementOf(e) != null
    }

    override fun doUnwrap(element: PsiElement, context: Context) {
        val statement = statementOf(element) ?: return
        context.extractBody(S.compositeChildren(element).last(), statement)
        context.delete(statement)
    }

    private fun statementOf(lambda: PsiElement): PsiElement? {
        var cur = lambda.parent
        while (cur != null && cur.elementType !== E.EXPRESSION_STATEMENT) {
            if (cur.elementType === E.CODE_BLOCK || cur.elementType === E.LAMBDA_EXPRESSION) return null
            cur = cur.parent
        }
        return cur?.takeIf { it.parent?.elementType === E.CODE_BLOCK }
    }
}
