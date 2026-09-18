package dev.jux.intellij.templates

import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.codeInsight.template.Expression
import com.intellij.codeInsight.template.ExpressionContext
import com.intellij.codeInsight.template.Macro
import com.intellij.codeInsight.template.Result
import com.intellij.codeInsight.template.TemplateContextType
import com.intellij.codeInsight.template.TextResult
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * One variable a template can use at some point: a local, a parameter, a
 * loop or catch binding, or a field of the enclosing type.
 *
 * [type] is the declared type as written (null for `var`), [initializer] the
 * text after `=` (null when there is none).
 */
data class JuxScopeVariable(val name: String, val type: String?, val initializer: String?) {
    /** Declared `T[]`, or initialized with `new T[n]` / `[a, b]`. */
    val isArray: Boolean
        get() = type?.trimEnd()?.endsWith("]") == true ||
            initializer?.let { it.startsWith("[") || (it.startsWith("new ") && '[' in it && '<' !in it.substringBefore('[')) } == true

    /**
     * Something a `for (var x : v)` can walk: an array, or a value of a
     * generic type (`Vec<T>`, `HashMap<K, V>`, ...). Written from the
     * declaration alone: the template only ranks suggestions, the checker
     * decides.
     */
    val isIterable: Boolean
        get() = isArray || type?.contains('<') == true ||
            initializer?.let { it.startsWith("new ") && '<' in it } == true
}

/**
 * The variables visible at a point, nearest first: what Java's
 * `variableOfType` / `iterableVariable` macros read from the resolver, read
 * here from the in-file PSI with the same scoping rules as
 * [dev.jux.intellij.resolve.JuxReference] (a local is visible after its
 * declaration, inner scopes shadow outer ones).
 */
object JuxScopeVariables {
    fun at(element: PsiElement): List<JuxScopeVariable> {
        val offset = element.textRange.startOffset
        val out = ArrayList<JuxScopeVariable>()
        val seen = HashSet<String>()
        fun add(decl: PsiElement) {
            val name = (decl as? JuxNamedElement)?.name ?: return
            if (seen.add(name)) out.add(JuxScopeVariable(name, typeText(decl), initializerText(decl)))
        }
        var scope: PsiElement? = element.parent
        while (scope != null && scope !is JuxFile) {
            when (scope.elementType) {
                E.CODE_BLOCK ->
                    // Nearest declaration first.
                    scope.children
                        .filter { it.elementType === E.LOCAL_VARIABLE && it.textRange.endOffset <= offset }
                        .asReversed()
                        .forEach(::add)
                E.FOR_EACH_STATEMENT, E.FOR_STATEMENT, E.CATCH_CLAUSE ->
                    scope.children
                        .filter { it.elementType === E.LOCAL_VARIABLE && it.textRange.endOffset <= offset }
                        .forEach(::add)
                E.LAMBDA_EXPRESSION, E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                    parameters(scope).forEach(::add)
                E.CLASS_BODY ->
                    scope.children
                        .filter { it.elementType === E.FIELD_DECLARATION || it.elementType === E.PROPERTY_DECLARATION }
                        .forEach(::add)
            }
            scope = scope.parent
        }
        return out
    }

    /** The parameters [callable] declares, in order. */
    fun parameters(callable: PsiElement): List<PsiElement> {
        val list = callable.children.firstOrNull { it.elementType === E.PARAMETER_LIST }
        val candidates = (list?.children?.toList() ?: emptyList()) + callable.children
        return candidates.filter { it.elementType === E.PARAMETER }
    }

    /** The method, constructor, operator or lambda [element] is inside. */
    fun enclosingCallable(element: PsiElement): PsiElement? {
        var p: PsiElement? = element.parent
        while (p != null && p !is JuxFile) {
            when (p.elementType) {
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION -> return p
            }
            p = p.parent
        }
        return null
    }

    private fun typeText(decl: PsiElement): String? =
        decl.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()

    private fun initializerText(decl: PsiElement): String? {
        var child = decl.node.firstChildNode
        while (child != null && child.elementType !== JuxTokenTypes.EQ) child = child.treeNext
        child = child?.treeNext ?: return null
        while (child != null && child.psi is com.intellij.psi.PsiWhiteSpace) child = child.treeNext
        return child?.text?.trim()
    }
}

/** Shared shape of the Jux macros: offered only in Jux template contexts. */
abstract class JuxMacro(private val macroName: String, private val presentable: String) : Macro() {
    override fun getName(): String = macroName
    override fun getPresentableName(): String = presentable
    override fun isAcceptableInContext(context: TemplateContextType): Boolean = context is JuxTemplateContextType

    /** The element the template is being written at. */
    protected fun at(context: ExpressionContext): PsiElement? = context.psiElementAtStartOffset
}

/**
 * `juxClassName()`: the enclosing type's name, `""` in a free function.
 * Java's `className()`.
 */
class JuxClassNameMacro : JuxMacro("juxClassName", "juxClassName()") {
    override fun calculateResult(params: Array<out Expression>, context: ExpressionContext): Result? {
        val element = at(context) ?: return null
        var p: PsiElement? = element.parent
        while (p != null && p !is JuxTypeDeclaration) p = p.parent
        return TextResult((p as? JuxTypeDeclaration)?.name ?: "")
    }
}

/**
 * `juxMethodName()`: the enclosing method's name; a constructor's is its
 * type's. Java's `methodName()`.
 */
class JuxMethodNameMacro : JuxMacro("juxMethodName", "juxMethodName()") {
    override fun calculateResult(params: Array<out Expression>, context: ExpressionContext): Result? {
        val element = at(context) ?: return null
        val callable = JuxScopeVariables.enclosingCallable(element) ?: return null
        return TextResult((callable as? JuxNamedElement)?.name ?: callable.parent?.let { enclosingTypeName(it) } ?: "")
    }

    private fun enclosingTypeName(start: PsiElement): String? {
        var p: PsiElement? = start
        while (p != null && p !is JuxTypeDeclaration) p = p.parent
        return (p as? JuxTypeDeclaration)?.name
    }
}

/**
 * `juxQualifiedMethodName()`: `Type.method` inside a type, `method` in a free
 * function. What Java's `soutm` builds from `className()` and `methodName()`.
 */
class JuxQualifiedMethodNameMacro : JuxMacro("juxQualifiedMethodName", "juxQualifiedMethodName()") {
    override fun calculateResult(params: Array<out Expression>, context: ExpressionContext): Result? {
        val element = at(context) ?: return null
        val callable = JuxScopeVariables.enclosingCallable(element)
        var type: PsiElement? = (callable ?: element).parent
        while (type != null && type !is JuxTypeDeclaration) type = type.parent
        val typeName = (type as? JuxTypeDeclaration)?.name
        val method = (callable as? JuxNamedElement)?.name ?: typeName
        return TextResult(listOfNotNull(typeName, method.takeIf { it != typeName || callable == null }).joinToString("."))
    }
}

/**
 * `juxMainSignature()`: the entry point's signature for where it is written
 * (§E.1.2). A free `void main()` at the top of a file; in a type,
 * `public static void main()`, since a type's `main` must be static (E0326).
 */
class JuxMainSignatureMacro : JuxMacro("juxMainSignature", "juxMainSignature()") {
    override fun calculateResult(params: Array<out Expression>, context: ExpressionContext): Result? {
        val element = at(context) ?: return TextResult("void main()")
        val inType = JuxTemplateContextType.enclosingTypeBody(element) != null
        return TextResult(if (inType) "public static void main()" else "void main()")
    }
}

/**
 * `juxParametersFormat()`: the enclosing method's parameters as the body of
 * an interpolated string, `a = ${a}, b = ${b}`. What Java's `soutp` builds
 * with a groovy script.
 */
class JuxParametersFormatMacro : JuxMacro("juxParametersFormat", "juxParametersFormat()") {
    override fun calculateResult(params: Array<out Expression>, context: ExpressionContext): Result? {
        val element = at(context) ?: return null
        val callable = JuxScopeVariables.enclosingCallable(element) ?: return TextResult("")
        val names = JuxScopeVariables.parameters(callable).mapNotNull { (it as? JuxNamedElement)?.name }
        return TextResult(names.joinToString(", ") { "$it = \${$it}" })
    }
}

/**
 * Suggests the variables in scope that pass [accept], nearest first, and
 * picks the nearest. The base of `juxVariable()`, `juxIterableVariable()` and
 * `juxArrayVariable()`: Java's `variableOfType`, `iterableVariable` and
 * `arrayVariable`.
 */
abstract class JuxVariableMacro(name: String, private val accept: (JuxScopeVariable) -> Boolean) :
    JuxMacro(name, "$name()") {
    private fun candidates(context: ExpressionContext): List<JuxScopeVariable> =
        at(context)?.let { JuxScopeVariables.at(it) }?.filter(accept) ?: emptyList()

    override fun calculateResult(params: Array<out Expression>, context: ExpressionContext): Result? =
        candidates(context).firstOrNull()?.let { TextResult(it.name) }

    override fun calculateLookupItems(params: Array<out Expression>, context: ExpressionContext): Array<LookupElement>? {
        val found = candidates(context)
        if (found.size < 2) return null
        return found.map { v ->
            LookupElementBuilder.create(v.name).withTypeText(v.type ?: v.initializer?.take(30), true)
        }.toTypedArray()
    }
}

/** `juxVariable()`: any variable in scope. */
class JuxAnyVariableMacro : JuxVariableMacro("juxVariable", { true })

/** `juxIterableVariable()`: a variable a for-each can walk. */
class JuxIterableVariableMacro : JuxVariableMacro("juxIterableVariable", { it.isIterable })

/** `juxArrayVariable()`: an array variable. */
class JuxArrayVariableMacro : JuxVariableMacro("juxArrayVariable", { it.isArray })

/**
 * `juxElementName(collection)`: a name for one element of `collection`, the
 * way Java's `suggestVariableName()` names a loop variable: `items` gives
 * `item`, `entries` gives `entry`, `nameList` gives `name`. Anything it cannot
 * singularize gives `item`.
 */
class JuxElementNameMacro : JuxMacro("juxElementName", "juxElementName(collection)") {
    override fun calculateResult(params: Array<out Expression>, context: ExpressionContext): Result? {
        val collection = params.firstOrNull()?.calculateResult(context)?.toString()?.trim().orEmpty()
        return TextResult(elementName(collection))
    }

    companion object {
        fun elementName(collection: String): String {
            val base = collection.substringAfterLast('.')
            val stripped = listOf("List", "Array", "Set", "Vec", "Queue")
                .firstOrNull { base.length > it.length && base.endsWith(it) }
                ?.let { base.dropLast(it.length) }
            val single = stripped ?: when {
                base.endsWith("ies") && base.length > 3 -> base.dropLast(3) + "y"
                base.endsWith("sses") || base.endsWith("xes") || base.endsWith("ches") || base.endsWith("shes") ->
                    base.dropLast(2)
                base.endsWith("s") && !base.endsWith("ss") && base.length > 1 -> base.dropLast(1)
                else -> null
            }
            val name = single?.replaceFirstChar { it.lowercase() }
            return if (name.isNullOrEmpty() || name == base || !name[0].isJavaIdentifierStart()) "item" else name
        }
    }
}
