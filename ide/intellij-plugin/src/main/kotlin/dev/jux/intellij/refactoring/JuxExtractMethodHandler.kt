package dev.jux.intellij.refactoring

import com.intellij.openapi.actionSystem.DataContext
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.elementType
import com.intellij.refactoring.RefactoringActionHandler
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxLocalVariable
import dev.jux.intellij.psi.JuxParameter

/**
 * Extract Method (`Ctrl+Alt+M`): the selected statements, or the selected
 * expression, become a new method and the selection becomes a call to it.
 *
 * The data flow is Java's:
 *
 * - **inputs** are the locals and parameters the selection reads that are
 *   declared outside it; each becomes a parameter, in order of first use;
 * - an **output** is a local the selection declares or assigns that the code
 *   after it still reads; it becomes the return value. Java allows one and so
 *   does this: two outputs would need a tuple or a holder object, and choosing
 *   one silently would change the program.
 *
 * The new method sits right after the one the selection came from, is
 * `private` in a type and a plain function at the top of a file, and is
 * `static` when the code it came from has no `this`.
 *
 * Refused, with the reason, when the selection is not whole statements or one
 * whole expression, when it contains a `return` (the caller's flow would
 * change), a `break`/`continue` leaving it, more than one output, or a value
 * whose type cannot be read off the code: a guessed parameter type compiles
 * into a different program instead of asking.
 */
class JuxExtractMethodHandler : RefactoringActionHandler {

    override fun invoke(project: Project, editor: Editor?, file: PsiFile?, dataContext: DataContext?) {
        if (editor == null || file !is JuxFile) return
        val selection = editor.selectionModel
        if (!selection.hasSelection()) {
            refuse(project, editor, "Select the statements or the expression to extract.")
            return
        }
        val plan = try {
            analyze(file, selection.selectionStart, selection.selectionEnd)
        } catch (e: Refusal) {
            refuse(project, editor, e.message!!)
            return
        }
        val container = plan.container
        val name = JuxRefactoringInput.ask(
            project, TITLE, "Name of the new method:",
            JuxRefactoringUtil.uniqueMemberName(DEFAULT_NAME, container),
        ) { candidate ->
            JuxRefactoringInput.identifierProblem(candidate)
                ?: if (container.children.any { JuxRefactoringUtil.nameOf(it) == candidate && it.elementType in JuxRefactoringUtil.CALLABLE_KINDS }) {
                    "`$candidate` is already declared here."
                } else null
        } ?: return

        WriteCommandAction.writeCommandAction(project, file).withName(TITLE).run<RuntimeException> {
            JuxRefactoringUtil.applyEdits(
                project,
                listOf(
                    JuxRefactoringUtil.Edit(file, plan.replaced, plan.callText(name)),
                    JuxRefactoringUtil.Edit(file, TextRange.from(plan.insertAt, 0), plan.methodText(name)),
                ),
            )
        }
    }

    /** Not reachable: the action works on an editor selection, never on a tree selection. */
    override fun invoke(project: Project, elements: Array<out PsiElement>, dataContext: DataContext?) = Unit

    /** Why a selection cannot be extracted; its message is shown as is. */
    class Refusal(message: String) : Exception(message)

    /** Everything the edit needs, computed before anything is changed. */
    class Plan(
        /** The type body (or file) the new method is added to. */
        val container: PsiElement,
        /** Where the method text goes: just after the callable it came from. */
        val insertAt: Int,
        /** The range the call replaces. */
        val replaced: TextRange,
        /** The extracted statements (statement mode) or null (expression mode). */
        private val bodyText: String,
        private val expression: Boolean,
        private val parameters: List<Pair<String, String>>,
        private val returnType: String,
        /** The output local and whether its declaration moved into the new method. */
        private val output: Output?,
        private val modifiers: String,
    ) {
        fun callText(name: String): String {
            val call = "$name(${parameters.joinToString(", ") { it.second }})"
            return when {
                expression -> call
                output == null -> "$call;"
                output.declaredInside -> "${output.type} ${output.name} = $call;"
                else -> "${output.name} = $call;"
            }
        }

        fun methodText(name: String): String {
            val params = parameters.joinToString(", ") { "${it.first} ${it.second}" }
            val body = when {
                expression -> "return $bodyText;"
                output != null -> "$bodyText\nreturn ${output.name};"
                else -> bodyText
            }
            return "\n\n${modifiers}$returnType $name($params) {\n$body\n}"
        }
    }

    /** The one value the extracted code hands back. */
    data class Output(val name: String, val type: String, val declaredInside: Boolean)

    companion object {
        const val TITLE = "Extract Method"
        private const val DEFAULT_NAME = "extracted"

        private fun refuse(project: Project, editor: Editor, message: String) =
            JuxRefactoringInput.refuse(project, editor, TITLE, message)

        /** Work out what extracting `[start, end)` of [file] means, or throw [Refusal]. */
        fun analyze(file: PsiFile, start: Int, end: Int): Plan {
            val text = file.text
            var s = start
            var e = end
            while (s < e && text[s].isWhitespace()) s++
            while (e > s && text[e - 1].isWhitespace()) e--
            if (s == e) throw Refusal("Select the statements or the expression to extract.")

            val expression = selectedExpression(file, s, e)
            val statements = if (expression == null) selectedStatements(file, s, e) else null
            if (expression == null && statements == null) {
                throw Refusal("The selection must be one whole expression or whole statements of one block.")
            }
            val anchor = expression ?: statements!!.first()
            val callable = JuxRefactoringUtil.enclosingCallable(anchor)
                ?: throw Refusal("Only code inside a method or function can be extracted.")
            val range = TextRange(s, e)
            val inside = { el: PsiElement -> range.contains(el.textRange) }

            // `return` would return from the NEW method; the caller would carry on.
            if (statements != null) {
                for (st in statements) {
                    if (JuxRefactoringUtil.descendants(st, E.RETURN_STATEMENT).isNotEmpty()) {
                        throw Refusal("The selection contains a `return`. Extracting it would change what the method returns.")
                    }
                    for (jump in JuxRefactoringUtil.descendants(st, E.BREAK_STATEMENT) + JuxRefactoringUtil.descendants(st, E.CONTINUE_STATEMENT)) {
                        if (!jumpStaysInside(jump, range)) {
                            throw Refusal("The selection contains a `break` or `continue` that leaves it.")
                        }
                    }
                }
            }
            val roots: List<PsiElement> = statements ?: listOf(expression!!)

            // ---- data flow ---------------------------------------------------
            val body = JuxRefactoringUtil.body(callable) ?: throw Refusal("The method has no body.")
            val inputs = LinkedHashMap<PsiElement, String>() // declaration -> name, first use first
            val assignedInside = HashSet<PsiElement>()
            val declaredInside = LinkedHashSet<PsiElement>()
            for (root in roots) {
                for ((decl, use) in JuxRefactoringUtil.variableUses(root)) {
                    if (!body.textRange.contains(decl.textRange) && !isParameterOf(decl, callable)) continue
                    if (inside(decl)) continue
                    inputs.putIfAbsent(decl, JuxRefactoringUtil.nameOf(decl) ?: continue)
                    if (isWritten(use)) assignedInside.add(decl)
                }
                for (local in JuxRefactoringUtil.descendants(root, E.LOCAL_VARIABLE)) declaredInside.add(local)
            }
            // Reads after the selection (or anywhere in an enclosing loop, which
            // runs the code before the selection again).
            val laterDecls = laterUses(body, range, anchor)
            val outputs = (assignedInside + declaredInside).filter { it in laterDecls }
            if (outputs.size > 1) {
                val names = outputs.mapNotNull { JuxRefactoringUtil.nameOf(it) }.joinToString(", ") { "`$it`" }
                throw Refusal("The code after the selection reads $names. A method can return only one value.")
            }
            if (expression != null && outputs.isNotEmpty()) {
                throw Refusal("The expression assigns a variable the code after it reads.")
            }
            val output = outputs.firstOrNull()?.let { decl ->
                val type = JuxRefactoringUtil.typeTextOfDeclaration(decl)
                    ?: throw Refusal("Cannot determine the type of `${JuxRefactoringUtil.nameOf(decl)}`. Give it a written type first.")
                Output(JuxRefactoringUtil.nameOf(decl)!!, type, decl in declaredInside)
            }
            val parameters = inputs.map { (decl, name) ->
                val type = JuxRefactoringUtil.typeTextOfDeclaration(decl)
                    ?: throw Refusal("Cannot determine the type of `$name`. Give it a written type first.")
                type to name
            }
            val returnType = when {
                expression != null -> JuxRefactoringUtil.typeTextOf(expression)
                    ?: throw Refusal("Cannot determine the type of the selected expression.")
                output != null -> output.type
                else -> "void"
            }
            if (expression != null && returnType == "void") {
                // A void call is a statement: extract it as one.
                return analyze(file, statementAround(expression)?.textRange?.startOffset ?: s, statementAround(expression)?.textRange?.endOffset ?: e)
            }

            val topLevel = JuxRefactoringUtil.isTopLevel(callable)
            val modifiers = buildString {
                if (!topLevel) append("private ")
                if (!topLevel && JuxRefactoringUtil.isStaticContext(callable)) append("static ")
            }
            val bodyText = JuxRefactoringUtil.dedent(text.substring(s, e))
            return Plan(
                container = if (topLevel) file else callable.parent,
                insertAt = callable.textRange.endOffset,
                replaced = range,
                bodyText = bodyText,
                expression = expression != null,
                parameters = parameters,
                returnType = returnType,
                output = output,
                modifiers = modifiers,
            )
        }

        /** The expression covering exactly `[s, e)`, the outermost if several share it. */
        private fun selectedExpression(file: PsiFile, s: Int, e: Int): PsiElement? {
            var el: PsiElement? = file.findElementAt(s)
            var best: PsiElement? = null
            while (el != null && el !is PsiFile) {
                val r = el.textRange
                if (r.startOffset == s && r.endOffset == e && JuxRefactoringUtil.isExpression(el)) best = el
                if (r.startOffset < s || r.endOffset > e) break
                el = el.parent
            }
            // A whole expression statement reads as statements, not as an expression.
            if (best != null && best.parent?.elementType === E.EXPRESSION_STATEMENT &&
                best.parent.textRange.endOffset == e
            ) return null
            return best
        }

        /** The consecutive statements of one block covering `[s, e)`, or null. */
        private fun selectedStatements(file: PsiFile, s: Int, e: Int): List<PsiElement>? {
            var first: PsiElement = file.findElementAt(s) ?: return null
            while (first.parent != null && first.parent.elementType !== E.CODE_BLOCK) {
                first = first.parent
                if (first is PsiFile || first.textRange.startOffset != s) return null
            }
            if (first.textRange.startOffset != s) return null
            val out = ArrayList<PsiElement>()
            var cur: PsiElement? = first
            while (cur != null && cur.textRange.endOffset <= e) {
                if (cur !is PsiWhiteSpace && cur !is PsiComment) {
                    if (cur.elementType === T.RBRACE) return null
                    out.add(cur)
                }
                if (cur.textRange.endOffset == e) break
                cur = cur.nextSibling
            }
            if (out.isEmpty() || out.last().textRange.endOffset != e) return null
            return out
        }

        private fun isVariable(decl: PsiElement) = decl is JuxLocalVariable || decl is JuxParameter

        private fun isParameterOf(decl: PsiElement, callable: PsiElement) =
            decl is JuxParameter && JuxRefactoringUtil.parameterList(callable)?.let { it.textRange.contains(decl.textRange) } == true

        /** True when [ref] is the target of an assignment or an increment. */
        private fun isWritten(ref: PsiElement): Boolean {
            val parent = ref.parent ?: return false
            return when (parent.elementType) {
                E.ASSIGNMENT_EXPRESSION -> parent.firstChild === ref
                E.POSTFIX_EXPRESSION, E.UNARY_EXPRESSION ->
                    parent.node.findChildByType(T.PLUS_PLUS) != null || parent.node.findChildByType(T.MINUS_MINUS) != null
                else -> false
            }
        }

        /** The variables [body] uses in code that runs after the selection. */
        private fun laterUses(body: PsiElement, range: TextRange, anchor: PsiElement): Set<PsiElement> {
            val loop = enclosingLoop(anchor, body)
            return JuxRefactoringUtil.variableUses(body)
                .filter { (_, use) -> !range.contains(use.textRange) }
                .filter { (_, use) ->
                    use.textRange.startOffset >= range.endOffset || (loop != null && loop.textRange.contains(use.textRange))
                }
                .map { it.first }
                .toSet()
        }

        private val LOOPS = setOf(E.FOR_STATEMENT, E.FOR_EACH_STATEMENT, E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT)

        private fun enclosingLoop(element: PsiElement, stop: PsiElement): PsiElement? {
            var e: PsiElement? = element.parent
            while (e != null && e !== stop) {
                if (e.elementType in LOOPS) return e
                e = e.parent
            }
            return null
        }

        /** A `break`/`continue` whose loop (or switch) is itself inside [range]. */
        private fun jumpStaysInside(jump: PsiElement, range: TextRange): Boolean {
            var e: PsiElement? = jump.parent
            while (e != null && e !is PsiFile) {
                if (e.elementType in LOOPS || e.elementType === E.SWITCH_STATEMENT) return range.contains(e.textRange)
                e = e.parent
            }
            return false
        }

        private fun statementAround(expression: PsiElement): PsiElement? =
            expression.parent?.takeIf { it.elementType === E.EXPRESSION_STATEMENT }
    }
}
