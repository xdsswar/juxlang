package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.parser.JUX_REF_KW
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * **E0464** (JUX-MISSING-DEFS §M.14.2), mirrored from the compiler's
 * `check_final_not_reassigned`: a `final` or `const` binding is immutable, so
 * storing into it again is an error.
 *
 * The bindings it covers are the ones the compiler seeds and walks:
 *
 *  - a `final`/`const` parameter;
 *  - a `final`/`const` local, the one in a `for (…; …; …)` init clause
 *    included;
 *  - a `final`/`const` for-each binder, `for (final String? note : notes)`
 *    (§A.2.8, ERRATA E95), which the parser keeps inside its
 *    [E.LOCAL_VARIABLE] so [JuxAccess.modifiers] reads the word from it.
 *
 * A `ref` or `weak` binding is left out: on those `x = v` stores through the
 * cell or retargets the handle (§M.13.2), so `final ref` never trips E0464.
 *
 * The stores are a plain `x = e`, a compound `x += e` and `++x` / `x--`, the
 * last two because the parser desugars them into the same assignment the
 * check sees. Only a bare name counts: `this.x = e` is a field write and has
 * a code of its own. Which binding a name means is the resolver's answer, so
 * shadowing (an inner `var x`, a plain for-each binder `x`) un-finals the name
 * exactly as it does in the compiler.
 *
 * The message echoes the keyword that was written (A.2.2): telling someone
 * who wrote `const` to drop `final` names a word they never used.
 */
class JuxFinalReassignmentInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                val target = when (element.elementType) {
                    E.ASSIGNMENT_EXPRESSION -> JuxTypeEngine.expressionChildren(element).firstOrNull()
                    E.UNARY_EXPRESSION, E.POSTFIX_EXPRESSION ->
                        if (element.node.findChildByType(T.PLUS_PLUS) != null || element.node.findChildByType(T.MINUS_MINUS) != null) {
                            JuxTypeEngine.expressionChildren(element).firstOrNull()
                        } else null
                    else -> null
                } ?: return
                val name = JuxCodeFacts.stripParens(target)?.takeIf { it.elementType === E.REFERENCE_EXPRESSION } ?: return
                if (name.node.findChildByType(T.DOT) != null) return
                val decl = runCatching { JuxTypeEngine.resolveReferenceExpression(name) }.getOrNull() ?: return
                val word = finalWord(decl) ?: return
                holder.registerProblem(
                    element,
                    "cannot reassign `${name.text}`: it is a `$word` binding and is immutable (§M.14.2). " +
                        "Drop `$word`, or bind a new local. (E0464)",
                    ProblemHighlightType.GENERIC_ERROR,
                    DropFinalFix(word),
                )
            }
        }

    /** A local or parameter's written `final`/`const`, or null when it is reassignable. */
    private fun finalWord(decl: PsiElement): String? {
        if (decl.elementType !== E.LOCAL_VARIABLE && decl.elementType !== E.PARAMETER) return null
        val mods = JuxAccess.modifiers(decl)
        if ("final" !in mods && "const" !in mods) return null
        if (keywordLeaf(decl) { it === JUX_REF_KW || it === T.WEAK_KW } != null) return null
        return keywordLeaf(decl) { it === T.FINAL_KW || it === T.CONST_KW }?.text
    }

    /** Removes the `final`/`const` from the declaration the error points back to. */
    private class DropFinalFix(private val word: String) : LocalQuickFix {
        override fun getFamilyName(): String = "Drop '$word'"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val store = descriptor.psiElement ?: return
            val target = JuxTypeEngine.expressionChildren(store).firstOrNull() ?: return
            val name = JuxCodeFacts.stripParens(target) ?: return
            val decl = runCatching { JuxTypeEngine.resolveReferenceExpression(name) }.getOrNull() ?: return
            val kw = keywordLeaf(decl) { it === T.FINAL_KW || it === T.CONST_KW } ?: return
            val end = (kw.nextSibling as? PsiWhiteSpace)?.textRange?.endOffset ?: kw.textRange.endOffset
            JuxCodeFacts.edit(project, decl.containingFile, kw.textRange.startOffset, end, "")
        }
    }

    private companion object {
        /**
         * The first leading keyword of [decl] that [wanted] accepts: the words
         * written before its type or name, where [JuxAccess.modifiers] reads a
         * local's `final` from.
         */
        fun keywordLeaf(decl: PsiElement, wanted: (com.intellij.psi.tree.IElementType?) -> Boolean): PsiElement? {
            var c: PsiElement? = decl.firstChild
            while (c != null && c.elementType !== T.IDENTIFIER && c.elementType !== E.TYPE_REFERENCE) {
                if (wanted(c.elementType)) return c
                c = c.nextSibling
            }
            return null
        }
    }
}
