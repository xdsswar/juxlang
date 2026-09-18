package dev.jux.intellij.refactoring

import com.intellij.lang.Language
import com.intellij.lang.refactoring.InlineActionHandler
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.elementType
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxSubtypes

/**
 * Inline Method (`Ctrl+Alt+N` on a method): every in-project call is replaced
 * by the method's body, then the method is deleted.
 *
 * Arguments are substituted for the parameters when that cannot change the
 * program (a name or a literal, or a side-effect-free expression the body
 * reads once and never through an interpolation shorthand); otherwise the
 * argument goes into a local first, evaluated once and in order, as Java
 * does. A local of the body that would collide with a name at the call site is
 * renamed.
 *
 * Refused, with the reason, for: a method with no body; one that overrides or
 * is overridden (the call may not reach this body at run time); a recursive
 * one; a `return` anywhere but the last statement; a call on another object
 * when the body uses its own class's members unqualified (they would bind to
 * the caller's); and a void method called inside an expression.
 */
class JuxInlineMethodHandler : InlineActionHandler() {

    override fun isEnabledForLanguage(l: Language): Boolean = l === JuxLanguage

    override fun canInlineElement(element: PsiElement): Boolean = element is JuxMethodDeclaration

    override fun getActionName(element: PsiElement?): String = TITLE

    override fun inlineElement(project: Project, editor: Editor?, element: PsiElement) {
        val method = element as? JuxMethodDeclaration ?: return
        val plan = try {
            plan(method)
        } catch (e: JuxExtractMethodHandler.Refusal) {
            JuxRefactoringInput.refuse(project, editor, TITLE, e.message!!)
            return
        }
        WriteCommandAction.writeCommandAction(project).withName(TITLE).run<RuntimeException> {
            JuxRefactoringUtil.applyEdits(project, plan)
        }
    }

    companion object {
        const val TITLE = "Inline Method"

        /** The edits that inline every call of [method] and delete it, or throw a refusal. */
        internal fun plan(method: JuxMethodDeclaration): List<JuxRefactoringUtil.Edit> {
            val name = method.name ?: throw refusal("The method has no name.")
            val body = JuxRefactoringUtil.body(method) ?: throw refusal("`$name` has no body to inline.")
            val owner = JuxHierarchy.enclosingType(method)
            if (owner != null && JuxHierarchy.findSuperMethod(owner, name, JuxHierarchy.arity(method)) != null) {
                throw refusal("`$name` overrides another method. A call may run a different body at run time.")
            }
            if (JuxSubtypes.overridingMethods(method).isNotEmpty()) {
                throw refusal("`$name` is overridden. A call may run a different body at run time.")
            }
            if (JuxRefactoringUtil.descendants(body, E.CALL_EXPRESSION).any { JuxRefactoringUtil.resolve(it.firstChild) === method }) {
                throw refusal("`$name` calls itself. A recursive method cannot be inlined.")
            }
            val statements = JuxRefactoringUtil.statements(body)
            val returns = JuxRefactoringUtil.descendants(body, E.RETURN_STATEMENT)
            val tailReturn = statements.lastOrNull()?.takeIf { it.elementType === E.RETURN_STATEMENT }
            if (returns.any { it !== tailReturn }) {
                throw refusal("`$name` returns from more than one place. Only a `return` as the last statement can be inlined.")
            }
            val calls = JuxRefactoringUtil.callsOf(method)
            if (calls.isEmpty()) throw refusal("`$name` is never called. Delete it instead (Safe Delete).")
            if (calls.any { outer -> calls.any { it !== outer && outer.textRange.contains(it.textRange) } }) {
                throw refusal("A call of `$name` is an argument of another. Inline the inner call first (Extract Variable).")
            }

            val usesOwnMembers = owner != null && usesMembersOf(body, owner)
            val edits = ArrayList<JuxRefactoringUtil.Edit>()
            for (call in calls) edits += inlineCall(method, body, statements, tailReturn, call, owner, usesOwnMembers)
            edits += JuxRefactoringUtil.Edit(method.containingFile, deletionRange(method), "")
            return edits
        }

        private fun refusal(message: String) = JuxExtractMethodHandler.Refusal(message)

        /** The edits inlining one call. */
        private fun inlineCall(
            method: JuxMethodDeclaration,
            body: PsiElement,
            statements: List<PsiElement>,
            tailReturn: PsiElement?,
            call: PsiElement,
            owner: dev.jux.intellij.psi.JuxTypeDeclaration?,
            usesOwnMembers: Boolean,
        ): List<JuxRefactoringUtil.Edit> {
            val name = method.name!!
            val receiver = JuxRefactoringUtil.receiverText(call)
            if (usesOwnMembers) {
                val sameType = JuxHierarchy.enclosingType(call) === owner
                if (!sameType || (receiver != null && receiver != "this" && receiver != owner?.name)) {
                    throw refusal("`$name` uses members of `${owner?.name}` and is called from elsewhere. They would not resolve at the call site.")
                }
            }
            // The statement the call is part of, in its block.
            val statement = enclosingStatement(call)
                ?: throw refusal("A call of `$name` is not inside a block (a field initializer?).")
            val isWholeStatement = statement.elementType === E.EXPRESSION_STATEMENT && statement.firstChild === call
            val returnExpr = tailReturn?.children?.firstOrNull { JuxRefactoringUtil.isExpression(it) }
            if (returnExpr == null && !isWholeStatement) {
                throw refusal("`$name` returns nothing but a call of it is used as a value.")
            }

            // ---- parameters -> arguments or locals -----------------------------
            val params = JuxHierarchy.parameters(method)
            val args = JuxRefactoringUtil.arguments(call)
            val uses = JuxRefactoringUtil.variableUses(body)
            val block = statement.parent
            val takenAtCall = namesVisibleAt(statement) + JuxRefactoringUtil.statements(block).mapNotNull { JuxRefactoringUtil.nameOf(it) }
            val prologue = StringBuilder()
            val substitution = HashMap<PsiElement, String>() // declaration -> text it becomes
            val usedNames = HashSet(takenAtCall)
            params.forEachIndexed { i, param ->
                val arg = args.getOrNull(i)?.text?.trim() ?: return@forEachIndexed
                val paramUses = uses.filter { it.first === param }
                val inInterpolation = paramUses.any { it.second.elementType !== E.REFERENCE_EXPRESSION }
                val written = paramUses.any { isWritten(it.second) }
                val simple = SIMPLE.matches(arg)
                val safeToSubstitute = !written && (
                    simple || (!inInterpolation && paramUses.size <= 1 && !JuxExtractSupport.hasSideEffects(args[i]) &&
                        !containsCall(args[i]))
                    )
                if (safeToSubstitute) {
                    substitution[param] = if (simple || args[i].elementType in ATOMIC) arg else "($arg)"
                } else {
                    val local = fresh(JuxRefactoringUtil.nameOf(param) ?: "arg", usedNames)
                    val type = JuxRefactoringUtil.declaredTypeText(param) ?: "var"
                    prologue.append("$type $local = $arg;\n")
                    substitution[param] = local
                }
            }
            // Locals of the body that would clash at the call site.
            for (local in JuxRefactoringUtil.descendants(body, E.LOCAL_VARIABLE)) {
                val localName = JuxRefactoringUtil.nameOf(local) ?: continue
                if (localName in takenAtCall || localName in substitution.values) {
                    substitution[local] = fresh(localName, usedNames)
                } else {
                    usedNames.add(localName)
                }
            }

            // ---- the inlined text ------------------------------------------------
            val bodyStatements = if (tailReturn != null) statements.dropLast(1) else statements
            val renderedStatements = bodyStatements.joinToString("\n") { render(it, substitution) }
            val renderedReturn = returnExpr?.let { render(it, substitution) }
            val file = call.containingFile
            val out = ArrayList<JuxRefactoringUtil.Edit>()
            val head = prologue.toString() + if (renderedStatements.isNotEmpty()) "$renderedStatements\n" else ""
            when {
                isWholeStatement -> {
                    // The call was the whole statement: the body replaces it, and a
                    // result nobody reads is kept only if it could do something.
                    val keepResult = returnExpr != null &&
                        (JuxExtractSupport.hasSideEffects(returnExpr) || containsCall(returnExpr))
                    val tail = if (keepResult) "$renderedReturn;" else ""
                    out += JuxRefactoringUtil.Edit(file, statement.textRange, (head + tail).trimEnd().ifEmpty { ";" })
                }
                else -> {
                    if (head.isNotEmpty()) out += JuxRefactoringUtil.Edit(file, TextRange.from(statement.textRange.startOffset, 0), head)
                    val value = renderedReturn!!
                    val wrapped = if (returnExpr!!.elementType in ATOMIC || call.parent?.elementType in NO_PARENS_PARENTS) value else "($value)"
                    out += JuxRefactoringUtil.Edit(file, call.textRange, wrapped)
                }
            }
            return out
        }

        /** [element]'s text with each use of a substituted declaration replaced. */
        private fun render(element: PsiElement, substitution: Map<PsiElement, String>): String {
            val base = element.textRange.startOffset
            // Uses (one edit per interpolated token, all renames at once), then
            // the declarations of renamed locals.
            val edits = JuxRefactoringUtil.renameEdits(element, substitution).toMutableList()
            for ((decl, target) in substitution) {
                if (decl.elementType !== E.LOCAL_VARIABLE || !element.textRange.contains(decl.textRange)) continue
                (decl as? dev.jux.intellij.psi.JuxNamedElement)?.nameIdentifier?.let {
                    edits += JuxRefactoringUtil.Edit(element.containingFile, it.textRange, target)
                }
            }
            val sb = StringBuilder(element.text)
            for (edit in edits.distinctBy { it.range }.sortedByDescending { it.range.startOffset }) {
                val text = edit.parts.joinToString("") { (it as JuxRefactoringUtil.Part.Lit).text }
                sb.replace(edit.range.startOffset - base, edit.range.endOffset - base, text)
            }
            return JuxRefactoringUtil.dedent(sb.toString())
        }

        /** The statement of a block that [element] belongs to. */
        private fun enclosingStatement(element: PsiElement): PsiElement? {
            var e: PsiElement? = element
            while (e != null && e.parent != null) {
                if (e.parent.elementType === E.CODE_BLOCK) return e
                e = e.parent
            }
            return null
        }

        /** Names of the locals and parameters visible at [statement]. */
        private fun namesVisibleAt(statement: PsiElement): Set<String> {
            val out = HashSet<String>()
            var scope: PsiElement? = statement.parent
            while (scope != null && scope !is com.intellij.psi.PsiFile) {
                for (child in scope.children) {
                    if (child.elementType === E.LOCAL_VARIABLE || child.elementType === E.PARAMETER) JuxRefactoringUtil.nameOf(child)?.let(out::add)
                    if (child.elementType === E.PARAMETER_LIST) child.children.mapNotNull { JuxRefactoringUtil.nameOf(it) }.forEach(out::add)
                }
                scope = scope.parent
            }
            return out
        }

        /** True when [body] reaches its type's own members without a qualifier. */
        private fun usesMembersOf(body: PsiElement, owner: dev.jux.intellij.psi.JuxTypeDeclaration): Boolean {
            if (JuxRefactoringUtil.descendants(body, E.THIS_EXPRESSION).isNotEmpty()) return true
            val members = JuxHierarchy.allMembers(owner).toSet()
            return JuxRefactoringUtil.descendants(body, E.REFERENCE_EXPRESSION).any { JuxRefactoringUtil.resolve(it) in members }
        }

        private fun isWritten(use: PsiElement): Boolean {
            val parent = use.parent ?: return false
            return when (parent.elementType) {
                E.ASSIGNMENT_EXPRESSION -> parent.firstChild === use
                E.POSTFIX_EXPRESSION, E.UNARY_EXPRESSION ->
                    parent.node.findChildByType(T.PLUS_PLUS) != null || parent.node.findChildByType(T.MINUS_MINUS) != null
                else -> false
            }
        }

        private fun containsCall(e: PsiElement) =
            e.elementType === E.CALL_EXPRESSION || e.elementType === E.NEW_EXPRESSION ||
                JuxRefactoringUtil.descendants(e, E.CALL_EXPRESSION).isNotEmpty() ||
                JuxRefactoringUtil.descendants(e, E.NEW_EXPRESSION).isNotEmpty()

        private fun fresh(base: String, used: MutableSet<String>): String {
            var name = base
            var i = 1
            while (name in used) name = "$base${i++}"
            used.add(name)
            return name
        }

        private fun deletionRange(method: PsiElement): TextRange = JuxRefactoringUtil.memberDeletionRange(method)

        private val SIMPLE = Regex("^([A-Za-z_][A-Za-z0-9_]*|-?\\d[\\d_.]*[a-zA-Z]*|\"[^\"\\\\$]*\"|'[^']*'|true|false|null|this)$")

        private val ATOMIC = setOf(
            E.LITERAL_EXPRESSION, E.REFERENCE_EXPRESSION, E.PARENTHESIZED_EXPRESSION, E.CALL_EXPRESSION,
            E.INDEX_EXPRESSION, E.FIELD_ACCESS_EXPRESSION, E.NEW_EXPRESSION, E.THIS_EXPRESSION,
        )

        /** Places a value never needs parentheses in: its own statement, an argument, an initializer. */
        private val NO_PARENS_PARENTS = setOf(E.ARGUMENT_LIST, E.LOCAL_VARIABLE, E.RETURN_STATEMENT, E.EXPRESSION_STATEMENT)
    }
}
