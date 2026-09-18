package dev.jux.intellij.refactoring

import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import com.intellij.refactoring.RefactoringActionHandler
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxSubtypes

/**
 * A name for a value, the way Java's suggester reads one off the expression:
 * a name or a member access gives its last identifier, a `getX()` call gives
 * `x`, a `new T(...)` gives `t`, and anything else `value`.
 */
internal object JuxNameSuggester {
    fun suggest(expression: PsiElement): String {
        val raw = when (expression.elementType) {
            E.REFERENCE_EXPRESSION -> expression.text.trim()
            E.FIELD_ACCESS_EXPRESSION -> expression.lastChild?.text
            E.CALL_EXPRESSION -> {
                val callee = expression.firstChild
                val name = (if (callee?.elementType === E.FIELD_ACCESS_EXPRESSION) callee.lastChild else callee)?.text
                name?.removePrefix("get")?.removePrefix("is")?.takeIf { it.isNotEmpty() } ?: name
            }
            E.NEW_EXPRESSION -> expression.node.findChildByType(E.TYPE_REFERENCE)?.text?.substringBefore('<')
            E.PARENTHESIZED_EXPRESSION -> expression.children.firstOrNull { JuxRefactoringUtil.isExpression(it) }?.let { return suggest(it) }
            else -> null
        }?.trim()
        val name = raw?.takeIf { it.isNotEmpty() && it.all { c -> c.isLetterOrDigit() || c == '_' } }
            ?.replaceFirstChar { it.lowercase() }
        return if (name == null || JuxRefactoringInput.identifierProblem(name) != null) "value" else name
    }
}

/** Shared by the two handlers below: the expression, where it lives, and its type. */
private class Target(val expression: PsiElement, val callable: PsiElement, val body: PsiElement, val type: String)

private fun target(project: Project, editor: Editor, file: PsiFile, title: String): Target? {
    val expression = JuxExtractSupport.expressionAt(file, editor)
    if (expression == null) {
        JuxRefactoringInput.refuse(project, editor, title, "Select an expression. An assignment, an increment or a lambda cannot be moved.")
        return null
    }
    val callable = JuxRefactoringUtil.enclosingCallable(expression)
    val body = callable?.let { JuxRefactoringUtil.body(it) }
    if (callable == null || body == null) {
        JuxRefactoringInput.refuse(project, editor, title, "Select an expression inside a method body.")
        return null
    }
    val type = JuxRefactoringUtil.typeTextOf(expression)
    if (type == null) {
        JuxRefactoringInput.refuse(project, editor, title, "Cannot determine the type of the selected expression.")
        return null
    }
    return Target(expression, callable, body, type)
}

/**
 * Introduce Field (`Ctrl+Alt+F`): the selected expression becomes a private
 * field and its occurrences in the method read the field.
 *
 * Where Java asks where to initialize the field, this picks the only choice
 * that keeps the program the same: an expression that reads nothing of the
 * method (no locals, no parameters) initializes the field in its declaration;
 * one that does is assigned to the field right before the statement that used
 * it. A static context gives a static field.
 */
class JuxIntroduceFieldHandler : RefactoringActionHandler {

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        if (editor == null || file !is JuxFile) return
        val t = target(project, editor, file, TITLE) ?: return
        val classBody = JuxExtractSupport.enclosingClassBody(t.expression)
        if (classBody == null || JuxRefactoringUtil.isTopLevel(t.callable)) {
            JuxRefactoringInput.refuse(project, editor, TITLE, "A field needs an enclosing type. This function is at the top of the file.")
            return
        }
        val name = JuxRefactoringInput.ask(project, TITLE, "Name of the new field:", JuxRefactoringUtil.uniqueMemberName(JuxNameSuggester.suggest(t.expression), classBody)) { n ->
            JuxRefactoringInput.identifierProblem(n)
                ?: if (classBody.children.any { JuxRefactoringUtil.nameOf(it) == n }) "`$n` is already declared in this type." else null
        } ?: return

        val readsMethod = JuxRefactoringUtil.variableUses(t.expression).isNotEmpty()
        val static = if (JuxRefactoringUtil.isStaticContext(t.callable)) "static " else ""
        val exprText = t.expression.text.trim()
        val firstField = classBody.children.none { it.elementType === E.FIELD_DECLARATION || it.elementType === E.CONST_DECLARATION }
        // The first field of a type gets a blank line between it and the members after it.
        val separator = if (firstField) "\n" else ""
        val declaration = (if (readsMethod) "private $static${t.type} $name;\n" else "private $static${t.type} $name = $exprText;\n") + separator
        val edits = ArrayList<JuxRefactoringUtil.Edit>()
        for (occurrence in JuxExtractSupport.occurrencesOf(t.expression, t.body)) {
            edits += JuxRefactoringUtil.Edit(file, occurrence.textRange, name)
        }
        if (readsMethod) {
            val block = JuxExtractSupport.enclosingBlock(t.expression)
            val statement = block?.let { JuxExtractSupport.statementIn(it, t.expression) }
            if (statement == null) {
                JuxRefactoringInput.refuse(project, editor, TITLE, "Cannot find the statement to assign the field in.")
                return
            }
            edits += JuxRefactoringUtil.Edit(file, TextRange.from(statement.textRange.startOffset, 0), "$name = $exprText;\n")
        }
        edits += JuxRefactoringUtil.Edit(file, TextRange.from(fieldInsertionOffset(classBody), 0), declaration)
        WriteCommandAction.writeCommandAction(project, file).withName(TITLE).run<RuntimeException> {
            JuxRefactoringUtil.applyEdits(project, edits)
        }
    }

    override fun invoke(project: Project, elements: Array<out PsiElement>, dataContext: DataContext?) = Unit

    companion object {
        const val TITLE = "Introduce Field"

        /** After the last field, else right after the body's `{`. */
        fun fieldInsertionOffset(body: PsiElement): Int {
            val lastField = body.children.lastOrNull {
                it.elementType === E.FIELD_DECLARATION || it.elementType === E.CONST_DECLARATION
            }
            val text = body.containingFile.text
            if (lastField != null) {
                var at = lastField.textRange.endOffset
                if (at < text.length && text[at] == '\n') at++
                return at
            }
            val brace = body.node.findChildByType(T.LBRACE)?.textRange?.endOffset ?: (body.textRange.startOffset + 1)
            return if (brace < text.length && text[brace] == '\n') brace + 1 else brace
        }
    }
}

/**
 * Introduce Parameter (`Ctrl+Alt+P`): the selected expression becomes a new
 * last parameter of the method, its occurrences in the body read the
 * parameter, and every in-project call passes the expression.
 *
 * At a call the expression is written in the caller's terms: a parameter it
 * reads becomes that call's argument. It may not read a local of the method
 * (the caller has no such value), and when it reads the method's own members
 * every call has to be inside the same type on `this`, or the members would
 * mean the caller's. Methods in an override family are refused: every
 * override would need the parameter too, which is Change Signature's job.
 */
class JuxIntroduceParameterHandler : RefactoringActionHandler {

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        if (editor == null || file !is JuxFile) return
        val t = target(project, editor, file, TITLE) ?: return
        val method = t.callable as? JuxMethodDeclaration
        if (method == null) {
            JuxRefactoringInput.refuse(project, editor, TITLE, "A parameter can be introduced into a method or function, not a constructor or an operator.")
            return
        }
        val owner = JuxHierarchy.enclosingType(method)
        val methodName = method.name ?: return
        if (owner != null && (JuxHierarchy.findSuperMethod(owner, methodName, JuxHierarchy.arity(method)) != null ||
                JuxSubtypes.overridingMethods(method).isNotEmpty())
        ) {
            JuxRefactoringInput.refuse(project, editor, TITLE, "`$methodName` overrides or is overridden. Use Change Signature to change the whole family.")
            return
        }
        val params = JuxHierarchy.parameters(method)
        val uses = JuxRefactoringUtil.variableUses(t.expression)
        uses.firstOrNull { (decl, _) -> decl !in params }?.let { (decl, _) ->
            JuxRefactoringInput.refuse(project, editor, TITLE, "The expression reads the local `${JuxRefactoringUtil.nameOf(decl)}`, which a caller does not have.")
            return
        }
        val calls = JuxRefactoringUtil.callsOf(method)
        if (owner != null && readsMembers(t.expression, owner)) {
            val foreign = calls.firstOrNull { call ->
                val r = JuxRefactoringUtil.receiverText(call)
                JuxHierarchy.enclosingType(call) !== owner || (r != null && r != "this")
            }
            if (foreign != null) {
                JuxRefactoringInput.refuse(project, editor, TITLE, "The expression reads members of `${owner.name}` and `$methodName` is called on another object.")
                return
            }
        }

        val taken = (params.mapNotNull { JuxRefactoringUtil.nameOf(it) } +
            JuxRefactoringUtil.descendants(t.body, E.LOCAL_VARIABLE).mapNotNull { JuxRefactoringUtil.nameOf(it) }).toSet()
        var suggestion = JuxNameSuggester.suggest(t.expression)
        var i = 1
        val base = suggestion
        while (suggestion in taken) suggestion = "$base${i++}"
        val name = JuxRefactoringInput.ask(project, TITLE, "Name of the new parameter:", suggestion) { n ->
            JuxRefactoringInput.identifierProblem(n) ?: if (n in taken) "`$n` is already used in `$methodName`." else null
        } ?: return

        val edits = ArrayList<JuxRefactoringUtil.Edit>()
        for (occurrence in JuxExtractSupport.occurrencesOf(t.expression, t.body)) {
            edits += JuxRefactoringUtil.Edit(file, occurrence.textRange, name)
        }
        val list = JuxRefactoringUtil.parameterList(method) ?: return
        val close = list.lastChild.takeIf { it.elementType === T.RPAREN } ?: return
        edits += JuxRefactoringUtil.Edit(file, TextRange.from(close.textRange.startOffset, 0), if (params.isEmpty()) "${t.type} $name" else ", ${t.type} $name")
        for (call in calls) {
            val argList = JuxRefactoringUtil.argumentList(call) ?: continue
            val argClose = argList.lastChild.takeIf { it.elementType === T.RPAREN } ?: continue
            val value = inCallerTerms(t.expression, params, JuxRefactoringUtil.arguments(call), uses)
            val sep = if (JuxRefactoringUtil.arguments(call).isEmpty()) "" else ", "
            edits += JuxRefactoringUtil.Edit(call.containingFile, TextRange.from(argClose.textRange.startOffset, 0), sep + value)
        }
        WriteCommandAction.writeCommandAction(project).withName(TITLE).run<RuntimeException> {
            JuxRefactoringUtil.applyEdits(project, edits)
        }
    }

    override fun invoke(project: Project, elements: Array<out PsiElement>, dataContext: DataContext?) = Unit

    /** [expression]'s text with each parameter replaced by the call's argument. */
    private fun inCallerTerms(
        expression: PsiElement,
        params: List<PsiElement>,
        args: List<PsiElement>,
        uses: List<Pair<PsiElement, PsiElement>>,
    ): String {
        val base = expression.textRange.startOffset
        val sb = StringBuilder(expression.text)
        for ((decl, use) in uses.sortedByDescending { it.second.textRange.startOffset }) {
            val arg = args.getOrNull(params.indexOf(decl)) ?: continue
            val argText = arg.text.trim()
            val replacement = if (use.elementType === E.REFERENCE_EXPRESSION) {
                if (arg.elementType in ATOMIC) argText else "($argText)"
            } else {
                val raw = use.elementType === T.INTERP_RAW_STRING_LITERAL
                JuxChangeSignature.renameInInterpolation(use.text, JuxRefactoringUtil.nameOf(decl)!!, "($argText)", raw)
            }
            sb.replace(use.textRange.startOffset - base, use.textRange.endOffset - base, replacement)
        }
        return sb.toString().trim()
    }

    private fun readsMembers(expression: PsiElement, owner: dev.jux.intellij.psi.JuxTypeDeclaration): Boolean {
        if (JuxRefactoringUtil.descendants(expression, E.THIS_EXPRESSION).isNotEmpty()) return true
        val members = JuxHierarchy.allMembers(owner).toSet()
        return JuxRefactoringUtil.descendants(expression, E.REFERENCE_EXPRESSION).any { JuxRefactoringUtil.resolve(it) in members }
    }

    companion object {
        const val TITLE = "Introduce Parameter"

        private val ATOMIC = setOf(
            E.LITERAL_EXPRESSION, E.REFERENCE_EXPRESSION, E.PARENTHESIZED_EXPRESSION, E.CALL_EXPRESSION,
            E.INDEX_EXPRESSION, E.FIELD_ACCESS_EXPRESSION, E.NEW_EXPRESSION, E.THIS_EXPRESSION,
        )
    }
}
