package dev.jux.intellij.refactoring

import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxSubtypes

/**
 * Change Signature (`Ctrl+F6`), the part that does the work: given the new
 * name, return type and parameter list of a method, rewrite
 *
 * - the method itself and every method in its override family (the methods it
 *   overrides, up the hierarchy, and every override of those, down it), so the
 *   family keeps overriding each other;
 * - every in-project call of any of them: arguments reordered, dropped, and a
 *   new parameter's default value passed where it was added;
 * - a renamed parameter's uses in each body, interpolated uses included.
 *
 * Headless on purpose: [JuxChangeSignatureHandler] only gathers the answers.
 */
class JuxChangeSignature(
    /** The method the user invoked the refactoring on. */
    val method: PsiElement,
    val newName: String,
    /** The new return type text; null keeps each method's own. */
    val newReturnType: String?,
    val parameters: List<Parameter>,
) {
    /**
     * One parameter of the new signature. [oldIndex] is its position in the
     * old one, or -1 for a new parameter, whose [defaultValue] is what every
     * existing call passes for it.
     */
    data class Parameter(val oldIndex: Int, val name: String, val type: String, val defaultValue: String? = null)

    /** Every method whose signature has to change with [method]. */
    fun family(): List<PsiElement> {
        val out = LinkedHashSet<PsiElement>()
        // The top of the chain, so siblings overriding the same method come too.
        var top: PsiElement = method
        while (true) {
            val owner = JuxHierarchy.enclosingType(top) ?: break
            val name = JuxRefactoringUtil.nameOf(top) ?: break
            top = JuxHierarchy.findSuperMethod(owner, name, JuxHierarchy.arity(top)) ?: break
        }
        out.add(top)
        (top as? JuxMethodDeclaration)?.let { out.addAll(JuxSubtypes.overridingMethods(it)) }
        out.add(method)
        return out.toList()
    }

    /** Problems that make the change unsafe; empty when it can run. */
    fun problems(): List<String> {
        val out = ArrayList<String>()
        JuxRefactoringInput.identifierProblem(newName)?.let { out.add(it) }
        val names = parameters.map { it.name }
        names.groupBy { it }.filter { it.value.size > 1 }.keys.forEach { out.add("Two parameters are named `$it`.") }
        for (p in parameters) {
            JuxRefactoringInput.identifierProblem(p.name)?.let { out.add(it) }
            if (p.type.isBlank()) out.add("Parameter `${p.name}` needs a type.")
            if (p.oldIndex < 0 && p.defaultValue.isNullOrBlank()) {
                out.add("New parameter `${p.name}` needs a value to pass at the existing calls.")
            }
        }
        return out
    }

    /** Apply the change. Must run on the EDT; opens its own write command. */
    fun run(project: Project) {
        val edits = ArrayList<JuxRefactoringUtil.Edit>()
        for (m in family()) edits += declarationEdits(m)
        for (m in family()) for (call in JuxRefactoringUtil.callsOf(m)) edits += callEdits(call)
        if (method.elementType === E.CONSTRUCTOR_DECLARATION) {
            for (creation in constructions()) edits += callEdits(creation)
        }
        WriteCommandAction.writeCommandAction(project).withName("Change Signature").run<RuntimeException> {
            JuxRefactoringUtil.applyEdits(project, edits.distinct())
        }
    }

    // ---- declarations ------------------------------------------------------

    private fun declarationEdits(m: PsiElement): List<JuxRefactoringUtil.Edit> {
        val file = m.containingFile
        val out = ArrayList<JuxRefactoringUtil.Edit>()
        // Name.
        (m as? JuxNamedElement)?.nameIdentifier?.let { id ->
            if (id.text != newName) out += JuxRefactoringUtil.Edit(file, id.textRange, newName)
        }
        // Return type: the TYPE_REFERENCE before the name.
        if (newReturnType != null) {
            val nameOffset = (m as? JuxNamedElement)?.nameIdentifier?.textRange?.startOffset ?: Int.MAX_VALUE
            m.node.findChildByType(E.TYPE_REFERENCE)?.psi
                ?.takeIf { it.textRange.endOffset <= nameOffset && it.text.trim() != newReturnType }
                ?.let { out += JuxRefactoringUtil.Edit(file, it.textRange, newReturnType) }
        }
        // Parameters, keeping each kept parameter's own text (modifiers,
        // annotations, a default value) and changing only its type and name.
        val list = JuxRefactoringUtil.parameterList(m) ?: return out
        val old = JuxHierarchy.parameters(m)
        val rendered = parameters.joinToString(", ") { p ->
            val kept = old.getOrNull(p.oldIndex)
            if (kept == null) "${p.type} ${p.name}" else renderKept(kept, p)
        }
        out += JuxRefactoringUtil.Edit(file, list.textRange, "($rendered)")
        // Renamed parameters: their uses in the body, all at once so a swap
        // of two names does not chain.
        val body = JuxRefactoringUtil.body(m)
        if (body != null) {
            val renames = HashMap<PsiElement, String>()
            for (p in parameters) {
                val kept = old.getOrNull(p.oldIndex) ?: continue
                if (JuxRefactoringUtil.nameOf(kept) != p.name) renames[kept] = p.name
            }
            out += JuxRefactoringUtil.renameEdits(body, renames)
        }
        return out
    }

    /** A kept parameter's text with its type and name replaced, the rest (modifiers, default) as written. */
    private fun renderKept(param: PsiElement, p: Parameter): String {
        val text = param.text
        val base = param.textRange.startOffset
        val typeRef = param.node.findChildByType(E.TYPE_REFERENCE)?.psi
        val id = (param as? JuxNamedElement)?.nameIdentifier
        if (typeRef == null || id == null) return "${p.type} ${p.name}"
        val sb = StringBuilder(text)
        // Name first: it sits after the type, so the type's offsets stay valid.
        sb.replace(id.textRange.startOffset - base, id.textRange.endOffset - base, p.name)
        sb.replace(typeRef.textRange.startOffset - base, typeRef.textRange.endOffset - base, p.type)
        return sb.toString()
    }

    // ---- calls -------------------------------------------------------------

    /**
     * `new T(...)` expressions that call this constructor: the type's
     * references inside a NEW_EXPRESSION whose argument count matches (a type
     * with several constructors is told apart by arity, as the compiler's
     * overload resolution first does).
     */
    private fun constructions(): List<PsiElement> {
        val type = JuxHierarchy.enclosingType(method) ?: return emptyList()
        val arity = JuxHierarchy.arity(method)
        val scope = com.intellij.psi.search.GlobalSearchScope.projectScope(method.project)
        return com.intellij.psi.search.searches.ReferencesSearch.search(type, scope).findAll()
            .mapNotNull { ref -> ref.element.parent?.takeIf { it.elementType === E.NEW_EXPRESSION } }
            .filter { JuxRefactoringUtil.arguments(it).size == arity }
            .distinct()
    }

    private fun callEdits(call: PsiElement): List<JuxRefactoringUtil.Edit> {
        val file = call.containingFile
        val out = ArrayList<JuxRefactoringUtil.Edit>()
        val callee = call.firstChild
        val nameLeaf = if (callee?.elementType === E.FIELD_ACCESS_EXPRESSION) callee.lastChild else callee
        if (call.elementType === E.CALL_EXPRESSION && nameLeaf != null && nameLeaf.text != newName) {
            out += JuxRefactoringUtil.Edit(file, nameLeaf.textRange, newName)
        }
        val args = JuxRefactoringUtil.arguments(call)
        val list = JuxRefactoringUtil.argumentList(call) ?: return out
        // Each kept argument is a slice of the original, so an edit inside it
        // (a nested call to the same method, a renamed parameter in a
        // recursive call) still applies after the reorder.
        val parts = ArrayList<JuxRefactoringUtil.Part>()
        parts += JuxRefactoringUtil.Part.Lit("(")
        parameters.forEachIndexed { i, p ->
            if (i > 0) parts += JuxRefactoringUtil.Part.Lit(", ")
            val kept = if (p.oldIndex >= 0) args.getOrNull(p.oldIndex) else null
            parts += if (kept != null) JuxRefactoringUtil.Part.Slice(kept.textRange)
            else JuxRefactoringUtil.Part.Lit(p.defaultValue ?: "")
        }
        parts += JuxRefactoringUtil.Part.Lit(")")
        out += JuxRefactoringUtil.Edit(file, list.textRange, parts)
        return out
    }

    companion object {
        /**
         * [text] (an interpolated string token) with every use of [oldName] in
         * a `${…}` hole or a `$name` shorthand renamed. Literal text is left
         * alone: `"count: ${count}"` keeps its label.
         */
        fun renameInInterpolation(text: String, oldName: String, newName: String, raw: Boolean): String =
            renameInInterpolation(text, mapOf(oldName to newName), raw)

        /**
         * [renameInInterpolation] for several names at once, in ONE pass, so a
         * swap (`a` to `b` and `b` to `a`) does not chain.
         */
        fun renameInInterpolation(text: String, renames: Map<String, String>, raw: Boolean): String {
            if (renames.isEmpty()) return text
            val out = StringBuilder()
            var i = 0
            fun isIdent(c: Char) = c.isLetterOrDigit() || c == '_'
            // A bare identifier in a hole, not a member name after a `.`.
            val word = Regex("(?<![\\w.])[A-Za-z_]\\w*")
            while (i < text.length) {
                val c = text[i]
                if (!raw && c == '\\' && i + 1 < text.length) {
                    out.append(c).append(text[i + 1]); i += 2; continue
                }
                if (c == '$' && i + 1 < text.length && text[i + 1] == '{') {
                    var depth = 1
                    var j = i + 2
                    while (j < text.length && depth > 0) {
                        when (text[j]) { '{' -> depth++; '}' -> depth-- }
                        j++
                    }
                    val holeEnd = if (depth == 0) j - 1 else j
                    val hole = text.substring(i + 2, holeEnd)
                    val renamed = word.replace(hole) { m -> renames[m.value] ?: m.value }
                    out.append("\${").append(renamed)
                    if (depth == 0) out.append('}')
                    i = j
                    continue
                }
                if (c == '$' && i + 1 < text.length && (text[i + 1].isLetter() || text[i + 1] == '_')) {
                    var j = i + 1
                    while (j < text.length && isIdent(text[j])) j++
                    val name = text.substring(i + 1, j)
                    val target = renames[name]
                    if (target != null) {
                        // `$name` only takes a name; anything else needs a `${…}` hole.
                        if (target.all { isIdent(it) }) out.append('$').append(target)
                        else out.append("\${").append(target).append('}')
                    } else {
                        out.append('$').append(name)
                    }
                    i = j
                    continue
                }
                out.append(c)
                i++
            }
            return out.toString()
        }

        /** The current signature of [method], as the dialog's starting point. */
        fun current(method: PsiElement): List<Parameter> =
            JuxHierarchy.parameters(method).mapIndexed { i, p ->
                Parameter(i, JuxRefactoringUtil.nameOf(p) ?: "p$i", JuxRefactoringUtil.declaredTypeText(p) ?: "")
            }

        /** The written return type of [method], `void` included. */
        fun returnType(method: PsiElement): String? = JuxHierarchy.returnTypeText(method)
    }
}
