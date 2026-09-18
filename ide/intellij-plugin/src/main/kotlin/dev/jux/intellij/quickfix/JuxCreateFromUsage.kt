package dev.jux.intellij.quickfix

import com.intellij.codeInsight.template.TemplateBuilderImpl
import com.intellij.codeInsight.template.TemplateManager
import com.intellij.codeInsight.template.impl.ConstantNode
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileEditor.FileEditorManager
import com.intellij.openapi.fileEditor.OpenFileDescriptor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.SmartPointerManager
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.inspections.JuxCodeFacts
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * What the "Create … from usage" fixes share, modeled on Java's
 * `CreateFromUsageUtils`: reading a call's arguments into parameters,
 * guessing the type a new declaration needs from where it is used, and
 * handing the new code to the user as a live template so every guessed type
 * and name is one Tab away from being corrected.
 *
 * Jux has no universal `Object` type (JUX-TYPE-SYSTEM-ADDENDUM §T.1), so a
 * type the IDE cannot guess is written as the placeholder [UNKNOWN_TYPE], a
 * template stop the user fills in; nothing is ever written as a type the
 * program cannot compile with by accident.
 */
object JuxCreateFromUsage {

    /** The type written where the IDE cannot tell; always a template stop. */
    const val UNKNOWN_TYPE = "Type"

    /** One parameter to create: its name and type text (null when unknown). */
    data class Param(val name: String, val type: String?)

    /** The arguments of [call] as parameters: names from what is passed, types from its type. */
    fun parametersFor(call: PsiElement): List<Param> {
        val args = call.node.findChildByType(E.ARGUMENT_LIST)?.psi
            ?.let { JuxTypeEngine.expressionChildren(it) } ?: return emptyList()
        val used = HashSet<String>()
        return args.map { arg ->
            val type = typeText(JuxTypeEngine.typeOf(arg))
            var name = nameFor(arg, type)
            if (!used.add(name)) {
                var i = 2
                while (!used.add("$name$i")) i++
                name = "$name$i"
            }
            Param(name, type)
        }
    }

    /**
     * A parameter name for an argument, the way Java suggests one: a name
     * passed as-is keeps its name, a member read keeps the member's name, and
     * anything else is named after its type (`int` gives `i`, `String` gives
     * `s`, `Point` gives `point`).
     */
    private fun nameFor(arg: PsiElement, type: String?): String {
        val s = JuxCodeFacts.stripParens(arg) ?: arg
        when (s.elementType) {
            E.REFERENCE_EXPRESSION, E.FIELD_ACCESS_EXPRESSION ->
                JuxTypeEngine.memberName(s)?.takeIf { it.first().isLowerCase() }?.let { return it }
            E.CALL_EXPRESSION -> s.firstChild?.let { JuxTypeEngine.memberName(it) }?.let { callee ->
                // `getName()` → `name`, `size()` → `size`.
                val stripped = callee.removePrefix("get").takeIf { it.isNotEmpty() && it[0].isUpperCase() }
                return (stripped ?: callee).replaceFirstChar { it.lowercase() }
            }
        }
        val base = type?.substringBefore('<')?.substringBefore('[')?.trimEnd('?') ?: return "arg"
        if (base.isEmpty()) return "arg"
        // A primitive or `String` gets its initial (`i`, `s`); a class its own name.
        return if (base[0].isLowerCase() || base == "String") base.take(1).lowercase()
        else base.replaceFirstChar { it.lowercase() }
    }

    /** A known type's source text, or null for the unknown and the unwritable. */
    fun typeText(t: JuxType): String? = when (t) {
        is JuxType.Primitive, is JuxType.ClassType, is JuxType.ArrayType, is JuxType.Nullable, is JuxType.TypeVar -> {
            // An unknown part prints as `?`; only a trailing nullable `?` is real.
            val text = t.presentable()
            text.takeIf { '?' !in text.removeSuffix("?") && text != "?" }
        }
        else -> null
    }

    /**
     * The type a value written at [expr] must have, read from where it sits:
     * the declared type it initializes or is assigned to, `bool` in a
     * condition, the enclosing method's return type after `return`, the
     * parameter type of the argument slot it fills, or the other operand's
     * type in arithmetic. Null when the context says nothing.
     */
    fun expectedType(expr: PsiElement): String? {
        val parent = expr.parent ?: return null
        when (parent.elementType) {
            E.PARENTHESIZED_EXPRESSION -> return expectedType(parent)
            E.LOCAL_VARIABLE, E.FIELD_DECLARATION ->
                return parent.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()
            E.ASSIGNMENT_EXPRESSION -> {
                val sides = JuxTypeEngine.expressionChildren(parent)
                if (sides.getOrNull(1) === expr) return typeText(JuxTypeEngine.typeOf(sides[0]))
                if (sides.getOrNull(0) === expr) return sides.getOrNull(1)?.let { typeText(JuxTypeEngine.typeOf(it)) }
            }
            E.IF_STATEMENT, E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT ->
                if (JuxCodeFacts.conditionOf(parent) === expr) return "bool"
            E.CONDITIONAL_EXPRESSION ->
                if (JuxTypeEngine.firstExpressionChild(parent) === expr) return "bool"
            E.UNARY_EXPRESSION -> if (parent.firstChild?.elementType === T.BANG) return "bool"
            E.BINARY_EXPRESSION -> {
                val op = JuxCodeFacts.binaryOperator(parent)?.elementType
                if (op === T.AND_AND || op === T.OR_OR) return "bool"
                if (op in ARITHMETIC || op in COMPARISON) {
                    val other = JuxCodeFacts.operands(parent).firstOrNull { it !== expr } ?: return null
                    val t = JuxTypeEngine.typeOf(other)
                    if (t is JuxType.Primitive) return t.name
                }
            }
            E.RETURN_STATEMENT -> {
                val callable = PsiTreeUtil.findFirstParent(parent) { p ->
                    p.elementType === E.METHOD_DECLARATION || p.elementType === E.OPERATOR_DECLARATION ||
                        p.elementType === E.LAMBDA_EXPRESSION
                } ?: return null
                if (callable.elementType === E.LAMBDA_EXPRESSION) return null
                return JuxHierarchy.returnTypeText(callable)?.takeIf { it != "void" }
            }
            E.ARGUMENT_LIST -> {
                val call = parent.parent ?: return null
                val index = JuxTypeEngine.expressionChildren(parent).indexOf(expr)
                val target = resolveCallee(call) ?: return null
                val param = JuxHierarchy.parameters(target).getOrNull(index) ?: return null
                return param.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()
            }
        }
        return null
    }

    /** The declaration a call invokes, when the IDE can find it. */
    fun resolveCallee(call: PsiElement): PsiElement? {
        val callee = call.firstChild ?: return null
        val argCount = JuxTypeEngine.argumentCount(call)
        return when (callee.elementType) {
            E.REFERENCE_EXPRESSION -> JuxTypeEngine.resolveReferenceExpression(callee, argCount)
            E.FIELD_ACCESS_EXPRESSION -> JuxTypeEngine.resolveMemberAccess(callee, argCount)?.element
            else -> null
        }?.takeIf { it.elementType === E.METHOD_DECLARATION || it.elementType === E.CONSTRUCTOR_DECLARATION }
    }

    /** True when [element] runs in a static context: a `static` method, or a `static {}` block. */
    fun isInStaticContext(element: PsiElement): Boolean {
        var p: PsiElement? = element.parent
        while (p != null && p !is JuxFile) {
            when (p.elementType) {
                E.METHOD_DECLARATION, E.PROPERTY_DECLARATION, E.FIELD_DECLARATION -> return JuxHierarchy.hasModifier(p, "static")
                E.STATIC_BLOCK -> return true
                E.CONSTRUCTOR_DECLARATION, E.INIT_BLOCK -> return false
            }
            if (p is JuxTypeDeclaration) return false
            p = p.parent
        }
        return false
    }

    /** The member of a type body (or top-level declaration) that contains [element]. */
    fun enclosingMember(element: PsiElement): PsiElement? {
        var p: PsiElement? = element
        while (p != null) {
            val parent = p.parent ?: return null
            if (parent.elementType === E.CLASS_BODY || parent is JuxFile) return p
            p = parent
        }
        return null
    }

    /**
     * Writes [memberText] into [type]'s body, after a blank line as the Java
     * generators leave one: after [after] when it is a member of that body,
     * else after the last member (or the opening brace). Returns the new
     * member.
     */
    fun addMember(type: JuxTypeDeclaration, memberText: String, after: PsiElement?): PsiElement? {
        val body = type.node.findChildByType(E.CLASS_BODY)?.psi ?: return null
        val close = body.node.findChildByType(T.RBRACE)?.psi ?: return null
        val anchor = if (after != null && after.parent === body) after else {
            var last = close.prevSibling
            while (last is PsiWhiteSpace) last = last.prevSibling
            last ?: return null
        }
        val emptyBody = anchor.elementType === T.LBRACE
        val at = anchor.textRange.endOffset
        // The closing brace keeps a line of its own.
        val next = anchor.nextSibling
        val closeFollows = next === close || (next is PsiWhiteSpace && !next.text.contains('\n'))
        val text = (if (emptyBody) "\n" else "\n\n") + memberText + if (closeFollows) "\n" else ""
        val file = type.containingFile
        val range = JuxCodeFacts.edit(type.project, file, at, at, text)
        var e = JuxCodeFacts.elementIn(file, range) ?: return null
        while (e.parent != null && e.parent.elementType !== E.CLASS_BODY) e = e.parent
        return e
    }

    /** Writes [text] as a top-level declaration after [after] (or at the end of [file]). */
    fun addTopLevel(file: JuxFile, text: String, after: PsiElement?): PsiElement? {
        val at = after?.takeIf { it.parent === file }?.textRange?.endOffset ?: file.textLength
        val range = JuxCodeFacts.edit(file.project, file, at, at, "\n\n$text\n")
        var e = JuxCodeFacts.elementIn(file, range) ?: return null
        while (e.parent != null && e.parent !is JuxFile) e = e.parent
        return e
    }

    /**
     * Turns [container] (already in the file) into a live template: each
     * element in [stops] becomes a stop holding its current text, in order,
     * and the caret ends after [endAfter] (or at the end of [container]).
     * The editor for [container]'s file is opened if [editor] shows another.
     */
    fun runTemplate(project: Project, editor: Editor?, container: PsiElement, stops: List<PsiElement>, endAfter: PsiElement? = null) {
        val file = container.containingFile ?: return
        val targetEditor = editorFor(project, editor, file) ?: return
        val pdm = PsiDocumentManager.getInstance(project)
        pdm.doPostponedOperationsAndUnblockDocument(targetEditor.document)
        pdm.commitDocument(targetEditor.document)
        val pointer = SmartPointerManager.createPointer(container)
        val live = pointer.element ?: return
        if (stops.isEmpty()) {
            targetEditor.caretModel.moveToOffset((endAfter ?: live).textRange.endOffset)
            return
        }
        val builder = TemplateBuilderImpl(live)
        for (stop in stops) if (stop.isValid) builder.replaceElement(stop, ConstantNode(stop.text))
        if (endAfter != null && endAfter.isValid) builder.setEndVariableAfter(endAfter)
        targetEditor.caretModel.moveToOffset(live.textRange.startOffset)
        val template = builder.buildInlineTemplate()
        TemplateManager.getInstance(project).startTemplate(targetEditor, template)
    }

    /** An editor showing [file]: [editor] when it already does, else one opened for it. */
    private fun editorFor(project: Project, editor: Editor?, file: com.intellij.psi.PsiFile): Editor? {
        val vfile = file.virtualFile ?: return editor
        if (editor != null && PsiDocumentManager.getInstance(project).getPsiFile(editor.document) == file) return editor
        return FileEditorManager.getInstance(project).openTextEditor(OpenFileDescriptor(project, vfile), true)
    }

    /**
     * True when [decl]'s file is one the user edits: a project source, not a
     * generated stub. A read-only flag does not count against it; the fix
     * asks to make the file writable when it runs, as Java's does.
     */
    fun isEditable(decl: PsiElement): Boolean {
        val vfile = decl.containingFile?.virtualFile ?: return false
        if (vfile.name.endsWith(".jux.d")) return false
        return com.intellij.openapi.roots.ProjectFileIndex.getInstance(decl.project).isInContent(vfile)
    }

    /** The body a created method starts with (a Jux built-in, never a Java library call). */
    const val NOT_IMPLEMENTED_BODY = "throw new UnsupportedOperationException();"

    /** The name identifier leaves of each parameter's TYPE_REFERENCE and name, in order. */
    fun parameterStops(params: PsiElement?): List<PsiElement> {
        if (params == null) return emptyList()
        val out = ArrayList<PsiElement>()
        for (p in params.children) {
            if (p.elementType !== E.PARAMETER) continue
            p.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let(out::add)
            (p as? JuxNamedElement)?.nameIdentifier?.let(out::add)
        }
        return out
    }

    private val ARITHMETIC = setOf(T.PLUS, T.MINUS, T.STAR, T.SLASH, T.PERCENT)
    private val COMPARISON = setOf(T.LT, T.LE, T.GT, T.GE, T.EQ_EQ, T.NOT_EQ)
}
