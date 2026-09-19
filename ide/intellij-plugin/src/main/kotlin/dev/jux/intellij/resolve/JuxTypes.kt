package dev.jux.intellij.resolve

import com.intellij.openapi.progress.ProgressManager
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.util.Key
import com.intellij.openapi.util.RecursionManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.CachedValue
import com.intellij.psi.util.CachedValueProvider
import com.intellij.psi.util.CachedValuesManager
import com.intellij.psi.util.PsiModificationTracker
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.completion.JuxAutoImport
import dev.jux.intellij.highlight.JuxKeywords
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.psi.JuxTypeParameter

/**
 * The type of a Jux expression as the editor understands it.
 *
 * This is the plugin's semantic model: what completion offers after a dot,
 * what go-to-declaration lands on, what rename and find-usages treat as the
 * same symbol. It is built from the PSI of the files involved, the project's
 * own sources and the toolchain's generated `.jux.d` library stubs alike, so a
 * type written in another package or in the Rust standard library is as real
 * here as one in the current file.
 */
sealed class JuxType {
    /** A declared class, interface, enum, record or struct, with its type arguments. */
    data class ClassType(val decl: JuxTypeDeclaration, val args: List<JuxType>) : JuxType() {
        override fun presentable(): String =
            (decl.name ?: "?") + if (args.isEmpty()) "" else args.joinToString(", ", "<", ">") { it.presentable() }
    }

    /** A type parameter in scope; its members are its bound's. */
    data class TypeVar(val param: JuxTypeParameter, val bound: JuxType?) : JuxType() {
        override fun presentable(): String = param.name ?: "?"
    }

    data class ArrayType(val element: JuxType) : JuxType() {
        override fun presentable(): String = element.presentable() + "[]"
    }

    data class Nullable(val inner: JuxType) : JuxType() {
        override fun presentable(): String = inner.presentable() + "?"
    }

    /** `int`, `bool`, `String`, `void`: a type with no declaration to read members from. */
    data class Primitive(val name: String) : JuxType() {
        override fun presentable(): String = name
    }

    /** A type named as a value, `Auto.` or `Color.`: static access to its members. */
    data class Static(val decl: JuxTypeDeclaration) : JuxType() {
        override fun presentable(): String = decl.name ?: "?"
    }

    object Unknown : JuxType() {
        override fun presentable(): String = "?"
    }

    abstract fun presentable(): String
}

/** A member found on a type, with the type it was found through (for generic substitution). */
data class JuxMember(val element: PsiElement, val owner: JuxType.ClassType)

/**
 * Expression typing and name resolution over Jux PSI.
 *
 * Every question is answered from the tree, never from source text, and every
 * answer is cached until the PSI changes. Resolution follows the language's
 * scoping: locals before parameters before members (inherited included) before
 * the file's top-level declarations, then imported and same-package types, then
 * everything else the project and its libraries declare.
 */
object JuxTypeEngine {

    private val EXPR_TYPE_KEY: Key<CachedValue<JuxType>> = Key.create("jux.expression.type")
    private val DECL_TYPE_KEY: Key<CachedValue<JuxType>> = Key.create("jux.declaration.type")

    private val TYPE_DECLS = setOf(
        E.CLASS_DECLARATION, E.INTERFACE_DECLARATION, E.ENUM_DECLARATION,
        E.RECORD_DECLARATION, E.STRUCT_DECLARATION, E.ANNOTATION_DECLARATION,
        E.TYPE_ALIAS_DECLARATION,
    )

    private val MAP_TYPES = setOf("HashMap", "BTreeMap", "Map", "IndexMap")

    // ------------------------------------------------------------------ typing

    /** The type of [expr], or [JuxType.Unknown]. */
    fun typeOf(expr: PsiElement?): JuxType {
        if (expr == null) return JuxType.Unknown
        return CachedValuesManager.getManager(expr.project).getCachedValue(expr, EXPR_TYPE_KEY, {
            val computed = RecursionManager.doPreventingRecursion(expr, false) { computeType(expr) }
            CachedValueProvider.Result.create(computed ?: JuxType.Unknown, PsiModificationTracker.MODIFICATION_COUNT)
        }, false)
    }

    private fun computeType(expr: PsiElement): JuxType {
        ProgressManager.checkCanceled()
        return when (expr.elementType) {
            E.PARENTHESIZED_EXPRESSION -> typeOf(firstExpressionChild(expr))
            E.REFERENCE_EXPRESSION -> when (val target = resolveReferenceExpression(expr)) {
                null -> JuxType.Unknown
                is JuxTypeDeclaration -> JuxType.Static(target)
                is JuxTypeParameter -> JuxType.Unknown
                else -> if (target.elementType === E.LOCAL_VARIABLE || target.elementType === E.PARAMETER) {
                    narrowedType(expr, target) ?: declaredType(target)
                } else {
                    declaredType(target)
                }
            }
            E.THIS_EXPRESSION -> PsiTreeUtil.getParentOfType(expr, JuxTypeDeclaration::class.java)
                ?.let { selfType(it) } ?: JuxType.Unknown
            E.SUPER_EXPRESSION -> PsiTreeUtil.getParentOfType(expr, JuxTypeDeclaration::class.java)
                ?.let { supertypes(selfType(it)).firstOrNull() } ?: JuxType.Unknown
            E.FIELD_ACCESS_EXPRESSION -> {
                // `Iface.super` (a default method of that interface) and
                // `Outer.this`: a value of the named type, not the type.
                if (expr.node.findChildByType(T.SUPER_KW) != null || expr.node.findChildByType(T.THIS_KW) != null) {
                    return classOf(typeOf(firstExpressionChild(expr))) ?: JuxType.Unknown
                }
                val member = resolveMemberAccess(expr) ?: return JuxType.Unknown
                if (member.element.elementType === E.METHOD_DECLARATION) JuxType.Unknown
                else memberType(member)
            }
            E.CALL_EXPRESSION -> typeOfCall(expr)
            E.NEW_EXPRESSION -> {
                val ref = expr.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return JuxType.Unknown
                val base = typeOfTypeReference(ref)
                if (expr.node.findChildByType(T.LBRACKET) != null) JuxType.ArrayType(base) else base
            }
            E.CAST_EXPRESSION -> expr.node.findChildByType(E.TYPE_REFERENCE)?.psi
                ?.let { typeOfTypeReference(it) } ?: JuxType.Unknown
            E.INDEX_EXPRESSION -> elementTypeOf(typeOf(firstExpressionChild(expr)))
            E.CONDITIONAL_EXPRESSION -> expressionChildren(expr).getOrNull(1)?.let { typeOf(it) } ?: JuxType.Unknown
            E.ASSIGNMENT_EXPRESSION -> typeOf(firstExpressionChild(expr))
            E.UNARY_EXPRESSION -> {
                val operand = firstExpressionChild(expr)
                if (expr.firstChild?.elementType === T.BANG) JuxType.Primitive("bool") else typeOf(operand)
            }
            E.POSTFIX_EXPRESSION -> stripNullable(typeOf(firstExpressionChild(expr)))
            E.LITERAL_EXPRESSION -> literalType(expr)
            E.BINARY_EXPRESSION -> binaryType(expr)
            // `start..end` on a user type calls its `operator..` (§O.2.4) and
            // has that operator's return type; a primitive range stays unknown.
            E.RANGE_EXPRESSION -> JuxOperators.resolve(expr)?.let { returnType(it) } ?: JuxType.Unknown
            else -> JuxType.Unknown
        }
    }

    /**
     * The type a bare type test proves for a local or parameter at [ref]
     * (JUX-TYPE-SYSTEM-ADDENDUM §T.6.2): inside the then-branch of
     * `if (x => Dog)`, the right side of `x => Dog && ...`, and the first arm
     * of `x => Dog ? ... : ...`, `x` is a `Dog`. A test with a binder
     * (`x => Dog d`) narrows the binder, not `x`. Null when nothing narrows.
     */
    private fun narrowedType(ref: PsiElement, target: PsiElement): JuxType? {
        val name = (target as? JuxNamedElement)?.name ?: return null
        var child: PsiElement = ref
        var parent: PsiElement? = ref.parent
        while (parent != null && parent !is PsiFile) {
            when (parent.elementType) {
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION, E.LAMBDA_EXPRESSION ->
                    return null
                E.IF_STATEMENT, E.WHILE_STATEMENT -> {
                    val cond = firstExpressionChild(parent)
                    if (cond != null && cond !== child && isThenBranch(parent, cond, child)) {
                        testedType(cond, name)?.let { return it }
                    }
                }
                E.BINARY_EXPRESSION -> {
                    val ops = expressionChildren(parent)
                    if (ops.size == 2 && ops[1] === child && parent.node.findChildByType(T.AND_AND) != null) {
                        testedType(ops[0], name)?.let { return it }
                    }
                }
                E.CONDITIONAL_EXPRESSION -> {
                    val ops = expressionChildren(parent)
                    if (ops.size >= 2 && ops[1] === child) testedType(ops[0], name)?.let { return it }
                }
            }
            child = parent
            parent = parent.parent
        }
        return null
    }

    /** Whether [child] is the branch run when [cond] holds: the statement right after it. */
    private fun isThenBranch(stmt: PsiElement, cond: PsiElement, child: PsiElement): Boolean {
        var c: PsiElement? = cond.nextSibling
        while (c != null) {
            if (c === child) return true
            if (c.elementType === T.ELSE_KW) return false
            c = c.nextSibling
        }
        return false
    }

    /** The type `cond` proves for `name`: `name => T`, or either side of an `&&`. */
    private fun testedType(cond: PsiElement, name: String): JuxType? {
        if (cond.elementType === E.PARENTHESIZED_EXPRESSION) return firstExpressionChild(cond)?.let { testedType(it, name) }
        if (cond.elementType !== E.BINARY_EXPRESSION) return null
        if (cond.node.findChildByType(T.AND_AND) != null) {
            return expressionChildren(cond).firstNotNullOfOrNull { testedType(it, name) }
        }
        if (cond.node.findChildByType(T.FAT_ARROW) == null) return null
        val left = firstExpressionChild(cond) ?: return null
        if (left.elementType !== E.REFERENCE_EXPRESSION || memberName(left) != name) return null
        // A binder after the type (`x => Dog d`) narrows the binder instead.
        val typeRef = cond.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return null
        var after = typeRef.nextSibling
        while (after is com.intellij.psi.PsiWhiteSpace) after = after.nextSibling
        if (after != null && (after.elementType === T.IDENTIFIER || after.elementType === E.LOCAL_VARIABLE)) return null
        return typeOfTypeReference(typeRef)
    }

    private fun typeOfCall(call: PsiElement): JuxType {
        val callee = call.firstChild ?: return JuxType.Unknown
        val argCount = argumentCount(call)
        return when (callee.elementType) {
            E.FIELD_ACCESS_EXPRESSION -> {
                val member = resolveMemberAccess(callee, argCount) ?: return enumBuiltinCallType(callee)
                if (member.element.elementType === E.METHOD_DECLARATION) returnType(member) else JuxType.Unknown
            }
            E.REFERENCE_EXPRESSION -> when (val target = resolveReferenceExpression(callee, argCount)) {
                // `Point(1, 2)` on a record, or a type called like a constructor.
                is JuxTypeDeclaration -> selfType(target)
                null -> JuxType.Unknown
                else -> if (target.elementType === E.METHOD_DECLARATION) {
                    val owner = PsiTreeUtil.getParentOfType(target, JuxTypeDeclaration::class.java)
                    if (owner != null) returnType(JuxMember(target, selfType(owner)))
                    else typeOfDeclarationTypeRef(target)
                } else JuxType.Unknown
            }
            else -> JuxType.Unknown
        }
    }

    /**
     * `Color.fromName("red")`, `c.ordinal()`: a call of an enum's built-in
     * helper (§7.7.3), which no declaration backs. Unknown for anything else.
     */
    private fun enumBuiltinCallType(callee: PsiElement): JuxType {
        val name = memberName(callee) ?: return JuxType.Unknown
        val receiver = typeOf(firstExpressionChild(callee))
        val enum = classOf(receiver)?.decl ?: return JuxType.Unknown
        val static = stripNullable(receiver) is JuxType.Static
        val builtin = JuxEnumBuiltins.find(enum, name, static) ?: return JuxType.Unknown
        return JuxEnumBuiltins.returnType(enum, builtin, callee)
    }

    private fun literalType(expr: PsiElement): JuxType = when (expr.firstChild?.elementType) {
        T.STRING_LITERAL, T.RAW_STRING_LITERAL, T.INTERP_STRING_LITERAL, T.INTERP_RAW_STRING_LITERAL ->
            stringType(expr)
        T.INT_LITERAL -> JuxType.Primitive("int")
        T.FLOAT_LITERAL -> JuxType.Primitive("double")
        T.CHAR_LITERAL -> JuxType.Primitive("char")
        T.BOOL_LITERAL -> JuxType.Primitive("bool")
        else -> JuxType.Unknown
    }

    private fun stringType(context: PsiElement): JuxType =
        JuxTypeIndex.findType(context, "String")?.let { JuxType.ClassType(it, emptyList()) }
            ?: JuxType.Primitive("String")

    private fun binaryType(expr: PsiElement): JuxType {
        val operands = expressionChildren(expr)
        // The operator is the first token that is neither an operand nor space.
        val op = expr.node.getChildren(null).firstOrNull {
            it.psi !in operands && it.elementType != com.intellij.psi.TokenType.WHITE_SPACE &&
                it.elementType !in T.COMMENTS
        }?.elementType
        val left = typeOf(operands.getOrNull(0))
        // A user operator decides the type itself: `v * 2.0` is a `Vec2` and
        // `v * w` a `double` when Vec2 overloads `*` by operand (§O.2.3).
        JuxOperators.resolve(expr)?.let { member ->
            val declared = returnType(member)
            if (declared !is JuxType.Unknown) return declared
        }
        return when (op) {
            T.EQ_EQ, T.NOT_EQ, T.LT, T.LE, T.GT, T.GE, T.AND_AND, T.OR_OR, T.STRICT_EQ, T.STRICT_NOT_EQ ->
                JuxType.Primitive("bool")
            T.FAT_ARROW -> JuxType.Primitive("bool")
            T.PLUS -> {
                val right = typeOf(operands.getOrNull(1))
                if (isString(left) || isString(right)) stringType(expr) else left
            }
            else -> left
        }
    }

    private fun isString(t: JuxType): Boolean =
        (t is JuxType.Primitive && t.name.equals("String", ignoreCase = true)) ||
            (t is JuxType.ClassType && t.decl.name == "String")

    /** The element type of an indexable: an array's element, a map's value, a sequence's first argument. */
    fun elementTypeOf(t: JuxType): JuxType = when (val s = stripNullable(t)) {
        is JuxType.ArrayType -> s.element
        is JuxType.ClassType -> when {
            s.args.isEmpty() -> JuxType.Unknown
            s.decl.name in MAP_TYPES && s.args.size >= 2 -> s.args[1]
            else -> s.args[0]
        }
        else -> JuxType.Unknown
    }

    fun stripNullable(t: JuxType): JuxType = if (t is JuxType.Nullable) t.inner else t

    // ------------------------------------------------------------ declarations

    /** The type a value declaration introduces: a local, parameter, field, property, component, enum constant. */
    fun declaredType(decl: PsiElement): JuxType =
        CachedValuesManager.getManager(decl.project).getCachedValue(decl, DECL_TYPE_KEY, {
            val computed = RecursionManager.doPreventingRecursion(decl, false) { computeDeclaredType(decl) }
            CachedValueProvider.Result.create(computed ?: JuxType.Unknown, PsiModificationTracker.MODIFICATION_COUNT)
        }, false)

    private fun computeDeclaredType(decl: PsiElement): JuxType {
        when (decl.elementType) {
            E.ENUM_CONSTANT -> return PsiTreeUtil.getParentOfType(decl, JuxTypeDeclaration::class.java)
                ?.let { JuxType.ClassType(it, emptyList()) } ?: JuxType.Unknown
            E.METHOD_DECLARATION -> return typeOfDeclarationTypeRef(decl)
        }
        decl.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let { return typeOfTypeReference(it) }
        if (decl.elementType === E.LOCAL_VARIABLE) binderType(decl)?.let { return it }
        // `var x = <expr>` / a field or property initializer.
        val initializer = initializerOf(decl)
        if (initializer != null) return typeOf(initializer)
        // `for (var k : xs)` — the iterable's element type.
        val parent = decl.parent
        if (parent?.elementType === E.FOR_EACH_STATEMENT) {
            val iterable = parent.node.getChildren(null)
                .dropWhile { it.elementType !== T.COLON }
                .firstOrNull { it.psi != null && isExpression(it.psi) }?.psi
            return elementTypeOf(typeOf(iterable))
        }
        return JuxType.Unknown
    }

    private fun typeOfDeclarationTypeRef(decl: PsiElement): JuxType =
        decl.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let { typeOfTypeReference(it) } ?: JuxType.Unknown

    private fun initializerOf(decl: PsiElement): PsiElement? {
        var sawEq = false
        var c: PsiElement? = decl.firstChild
        while (c != null) {
            if (c.elementType === T.EQ) sawEq = true
            else if (sawEq && isExpression(c)) return c
            c = c.nextSibling
        }
        return null
    }

    /**
     * The type of a name a pattern binds, or null when [local] is no binder:
     *  - `x => Dog d`: `d` is a `Dog`;
     *  - `case Circle(var r)` / `var Pt(a, b) = p`: the record component the
     *    binder sits in the place of;
     *  - `case var v`: the switch subject.
     */
    private fun binderType(local: PsiElement): JuxType? {
        val parent = local.parent ?: return null
        when (parent.elementType) {
            E.BINARY_EXPRESSION -> return parent.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let { typeOfTypeReference(it) }
            E.PATTERN -> {
                val outer = parent.parent ?: return null
                if (outer.elementType === E.SWITCH_CASE) {
                    // `case var v ->` binds the subject itself.
                    val switch = outer.parent ?: return null
                    return typeOf(firstExpressionChild(switch))
                }
                if (outer.elementType !== E.PATTERN) return null
                val head = patternHeadName(outer) ?: return null
                val index = outer.children.filter { it.elementType === E.PATTERN }.indexOf(parent)
                return componentType(outer, head, index)
            }
            E.DESTRUCTURING_DECLARATION -> {
                // The binders sit flat among the tokens: count the commas back
                // to the `(` that opens this binder's list, whose head names
                // the record.
                var depth = 0
                var index = 0
                var c: PsiElement? = local.prevSibling
                while (c != null) {
                    when (c.elementType) {
                        T.RPAREN -> depth++
                        T.LPAREN -> if (depth == 0) {
                            var head = c.prevSibling
                            while (head is com.intellij.psi.PsiWhiteSpace) head = head.prevSibling
                            if (head?.elementType !== E.TYPE_REFERENCE) return null
                            val name = patternHeadName(head) ?: return null
                            return componentType(head, name, index)
                        } else depth--
                        T.COMMA -> if (depth == 0) index++
                    }
                    c = c.prevSibling
                }
                return null
            }
        }
        return null
    }

    /** The last name before a pattern's `(`: `Circle` of `geo.Circle(var r)`. */
    private fun patternHeadName(pattern: PsiElement): String? {
        var name: String? = null
        var c = pattern.node.firstChildNode
        while (c != null && c.elementType !== T.LPAREN) {
            if (c.elementType === T.IDENTIFIER) name = c.text
            c = c.treeNext
        }
        return name
    }

    /** The type of component [index] of the record named [recordName], seen from [context]. */
    private fun componentType(context: PsiElement, recordName: String, index: Int): JuxType? {
        if (index < 0) return null
        val record = resolveTypeName(context, recordName) as? JuxTypeDeclaration ?: return null
        val component = JuxHierarchy.recordComponents(record).getOrNull(index) ?: return null
        return declaredType(component)
    }

    // ------------------------------------------------------------ type refs

    /** The type a written TYPE_REFERENCE denotes, resolved from where it is written. */
    fun typeOfTypeReference(ref: PsiElement): JuxType {
        val node = ref.node
        var name: String? = null
        var c = node.firstChildNode
        while (c != null && c.elementType !== E.TYPE_ARGUMENT_LIST && c.elementType !== T.LBRACKET) {
            if (c.elementType === T.IDENTIFIER || c.elementType === T.VOID_KW) name = c.text
            c = c.treeNext
        }
        if (name == null) {
            name = node.firstChildNode?.text ?: return JuxType.Unknown
        }
        var type: JuxType = when {
            name == "void" -> JuxType.Primitive("void")
            name == "String" || name == "string" ->
                JuxTypeIndex.findType(ref, "String")?.let { JuxType.ClassType(it, emptyList()) } ?: JuxType.Primitive("String")
            name in JuxKeywords.PRIMITIVES -> JuxType.Primitive(name)
            else -> {
                val args = node.findChildByType(E.TYPE_ARGUMENT_LIST)?.psi?.children
                    ?.filter { it.elementType === E.TYPE_REFERENCE }
                    ?.map { typeOfTypeReference(it) }
                    ?: emptyList()
                when (val target = resolveTypeName(ref, name, qualifier(ref))) {
                    is JuxTypeParameter -> JuxType.TypeVar(target, boundOf(target))
                    is JuxTypeDeclaration -> JuxType.ClassType(target, args)
                    else -> JuxType.Unknown
                }
            }
        }
        // Suffixes after the name: `[]` arrays, `?` nullable. Pointers keep the pointee.
        var suffix = node.firstChildNode
        while (suffix != null) {
            when (suffix.elementType) {
                T.LBRACKET -> type = JuxType.ArrayType(type)
                T.QUESTION -> type = JuxType.Nullable(type)
            }
            suffix = suffix.treeNext
        }
        return type
    }

    private fun qualifier(ref: PsiElement): String? {
        val ids = ref.node.getChildren(null)
            .takeWhile { it.elementType !== E.TYPE_ARGUMENT_LIST && it.elementType !== T.LBRACKET }
            .filter { it.elementType === T.IDENTIFIER }
            .map { it.text }
        return if (ids.size > 1) ids.dropLast(1).joinToString(".") else null
    }

    /** The declared upper bound of a type parameter: `<T extends Auto>` → `Auto`. */
    fun boundOf(param: JuxTypeParameter): JuxType? {
        // The bound sits inside the parameter node, or as the siblings after
        // it up to the next parameter (`<`, `T`, `extends`, `Auto`, `,`, …).
        param.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let { return typeOfTypeReference(it) }
        var c = param.node.treeNext
        var sawExtends = false
        while (c != null) {
            when (c.elementType) {
                E.TYPE_PARAMETER, T.COMMA, T.GT -> return null
                T.EXTENDS_KW, T.COLON -> sawExtends = true
                E.TYPE_REFERENCE -> if (sawExtends) return typeOfTypeReference(c.psi)
            }
            c = c.treeNext
        }
        return null
    }

    /**
     * What a type name written at [context] refers to: a type parameter of an
     * enclosing declaration, a type in this file, an imported type, a type in
     * the same package, then any type of that name in the project or its
     * libraries. [qualifier] is a written package path (`some.Truck`).
     */
    fun resolveTypeName(context: PsiElement, name: String, qualifier: String? = null): PsiElement? {
        if (qualifier != null) {
            findTypeByFqn(context, qualifier, name)?.let { return it }
        } else {
            var scope: PsiElement? = context.parent
            while (scope != null && scope !is PsiFile) {
                if (scope.elementType in TYPE_DECLS || scope.elementType === E.METHOD_DECLARATION ||
                    scope.elementType === E.CONSTRUCTOR_DECLARATION
                ) {
                    scope.node.findChildByType(E.TYPE_PARAMETER_LIST)?.psi?.children
                        ?.firstOrNull { it is JuxTypeParameter && it.name == name }
                        ?.let { return it }
                }
                scope = scope.parent
            }
        }
        return JuxTypeIndex.findType(context, name)
    }

    /** The type named [name] declared in package [pkg], through the declaration index. */
    fun findTypeByFqn(context: PsiElement, pkg: String, name: String): JuxTypeDeclaration? {
        val project = context.project
        if (DumbService.isDumb(project)) return null
        return JuxTypeIndex.typesNamed(project, name).firstOrNull { JuxAutoImport.packageOf(it) == pkg }
    }

    // ------------------------------------------------------------ classes

    /** A type declaration seen from inside itself: its own type parameters as arguments. */
    fun selfType(decl: JuxTypeDeclaration): JuxType.ClassType {
        val params = decl.node.findChildByType(E.TYPE_PARAMETER_LIST)?.psi?.children
            ?.filterIsInstance<JuxTypeParameter>()
            ?.map { JuxType.TypeVar(it, boundOf(it)) }
            ?: emptyList()
        return JuxType.ClassType(decl, params)
    }

    /** The class a type's members come from, when it has one. */
    fun classOf(t: JuxType): JuxType.ClassType? = when (val s = stripNullable(t)) {
        is JuxType.ClassType -> s
        is JuxType.TypeVar -> s.bound?.let { classOf(it) }
        is JuxType.Static -> JuxType.ClassType(s.decl, emptyList())
        is JuxType.Primitive -> null
        else -> null
    }

    /** The substitution a class type applies to its declaration's type parameters. */
    fun substitution(ct: JuxType.ClassType): Map<String, JuxType> {
        val names = ct.decl.node.findChildByType(E.TYPE_PARAMETER_LIST)?.psi?.children
            ?.filterIsInstance<JuxTypeParameter>()?.mapNotNull { it.name } ?: return emptyMap()
        val out = HashMap<String, JuxType>()
        for ((i, n) in names.withIndex()) {
            ct.args.getOrNull(i)?.let { out[n] = it }
        }
        return out
    }

    /** Replace type variables in [t] through [subst]. */
    fun substitute(t: JuxType, subst: Map<String, JuxType>): JuxType {
        if (subst.isEmpty()) return t
        return when (t) {
            is JuxType.TypeVar -> subst[t.param.name] ?: t
            is JuxType.ClassType -> JuxType.ClassType(t.decl, t.args.map { substitute(it, subst) })
            is JuxType.ArrayType -> JuxType.ArrayType(substitute(t.element, subst))
            is JuxType.Nullable -> JuxType.Nullable(substitute(t.inner, subst))
            else -> t
        }
    }

    /** The direct supertypes of [ct], with [ct]'s type arguments carried into them. */
    fun supertypes(ct: JuxType.ClassType): List<JuxType.ClassType> {
        val subst = substitution(ct)
        return JuxHierarchy.supertypeReferences(ct.decl).mapNotNull { (ref, _) ->
            classOf(substitute(typeOfTypeReference(ref), subst))
        }
    }

    /** [ct] and every supertype, nearest first, each carrying its substitution. */
    fun typeAndSupertypes(ct: JuxType.ClassType): List<JuxType.ClassType> {
        val out = ArrayList<JuxType.ClassType>()
        val seen = HashSet<JuxTypeDeclaration>()
        val queue = ArrayDeque<JuxType.ClassType>()
        queue.add(ct)
        while (queue.isNotEmpty()) {
            val t = queue.removeFirst()
            if (!seen.add(t.decl)) continue
            out.add(t)
            queue.addAll(supertypes(t))
        }
        return out
    }

    /** Members of [t] reachable after a dot, nearest declaration first, overrides once. */
    fun membersOf(t: JuxType): List<JuxMember> {
        val ct = classOf(t) ?: return emptyList()
        val out = ArrayList<JuxMember>()
        val seen = HashSet<String>()
        for (owner in typeAndSupertypes(ct)) {
            for (m in JuxHierarchy.allMembersDeclaredIn(owner.decl)) {
                val name = (m as? JuxNamedElement)?.name ?: continue
                val key = if (m.elementType === E.METHOD_DECLARATION) "$name/${JuxHierarchy.arity(m)}" else name
                if (seen.add(key)) out.add(JuxMember(m, owner))
            }
        }
        return out
    }

    /**
     * Every member of [t] named [name], same-arity overloads included.
     *
     * [membersOf] keeps one method per name and arity, which is right for a
     * completion list but wrong where overloads matter: Jux overloads on
     * parameter types, so `print(int)` and `print(String)` are both real, and
     * a caller that must not guess between them (parameter hints) needs both.
     */
    fun membersOfAllOverloads(t: JuxType, name: String): List<PsiElement> {
        val ct = classOf(t) ?: return emptyList()
        val out = ArrayList<PsiElement>()
        for (owner in typeAndSupertypes(ct)) {
            JuxHierarchy.allMembersDeclaredIn(owner.decl).filterTo(out) { (it as? JuxNamedElement)?.name == name }
        }
        return out
    }

    /** Whether a member is reached through the type rather than an instance. */
    fun isStaticMember(m: PsiElement): Boolean =
        m.elementType === E.ENUM_CONSTANT || JuxHierarchy.hasModifier(m, "static") ||
            m.elementType === E.CONST_DECLARATION

    /** A field's, property's or component's type, as seen through its owner. */
    fun memberType(member: JuxMember): JuxType {
        val raw = declaredType(member.element)
        return substitute(raw, substitution(member.owner))
    }

    /** A method's return type, as seen through its owner. */
    fun returnType(member: JuxMember): JuxType {
        val ref = member.element.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return JuxType.Unknown
        // A generated stub writes a constructor-like return as `Self`.
        if (ref.text.trim() == "Self") return member.owner
        return substitute(typeOfTypeReference(ref), substitution(member.owner))
    }

    // ------------------------------------------------------------ resolution

    /**
     * What `qualifier.name` refers to: a member of the qualifier's type. With
     * [argCount], a method of that arity is preferred among overloads.
     */
    fun resolveMemberAccess(access: PsiElement, argCount: Int? = null): JuxMember? {
        val qualifier = firstExpressionChild(access) ?: return null
        val name = memberName(access) ?: return null
        val qualifierType = typeOf(qualifier)
        val static = stripNullable(qualifierType) is JuxType.Static
        val candidates = membersOf(qualifierType).filter { (it.element as? JuxNamedElement)?.name == name }
        if (candidates.isEmpty()) return null
        val byStatic = candidates.filter { isStaticMember(it.element) == static }.ifEmpty { candidates }
        if (argCount != null) {
            byStatic.firstOrNull {
                it.element.elementType === E.METHOD_DECLARATION && JuxHierarchy.arity(it.element) == argCount
            }?.let { return it }
            byStatic.firstOrNull { it.element.elementType === E.METHOD_DECLARATION }?.let { return it }
        }
        return byStatic.first()
    }

    /** The member name of a field access: its last identifier (or keyword used as a name). */
    fun memberName(access: PsiElement): String? {
        var last: String? = null
        var c = access.node.firstChildNode
        while (c != null) {
            if (c.elementType === T.IDENTIFIER || T.KEYWORDS.contains(c.elementType)) last = c.text
            c = c.treeNext
        }
        return last
    }

    /**
     * What a bare name refers to at [ref]: a local, parameter, member (inherited
     * included), top-level declaration, native function, or a type.
     */
    fun resolveReferenceExpression(ref: PsiElement, argCount: Int? = null, nameOverride: String? = null): PsiElement? {
        val name = nameOverride ?: memberName(ref) ?: return null
        val offset = ref.textRange.startOffset
        var child: PsiElement = ref
        var scope: PsiElement? = ref.parent
        while (scope != null) {
            ProgressManager.checkCanceled()
            // Pattern binders: `case Circle(var r) -> r`, `if (x => Dog d) d`.
            dev.jux.intellij.psi.JuxLocals.bindersInScope(scope, child)
                .firstOrNull { (it as? JuxNamedElement)?.name == name }?.let { return it }
            when (scope.elementType) {
                E.CODE_BLOCK -> for (child in dev.jux.intellij.psi.JuxLocals.blockLocals(scope)) {
                    if (child.elementType === E.LOCAL_VARIABLE && child.textRange.startOffset < offset &&
                        (child as? JuxNamedElement)?.name == name
                    ) return child
                }
                E.FOR_EACH_STATEMENT, E.FOR_STATEMENT, E.CATCH_CLAUSE -> scope.children.firstOrNull {
                    it.elementType === E.LOCAL_VARIABLE && (it as? JuxNamedElement)?.name == name
                }?.let { return it }
                E.LAMBDA_EXPRESSION -> {
                    val list = scope.children.firstOrNull { it.elementType === E.PARAMETER_LIST }
                    ((list?.children?.toList() ?: emptyList()) + scope.children).firstOrNull {
                        it.elementType === E.PARAMETER && (it as? JuxNamedElement)?.name == name
                    }?.let { return it }
                }
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                    scope.node.findChildByType(E.PARAMETER_LIST)?.psi?.children?.firstOrNull {
                        it.elementType === E.PARAMETER && (it as? JuxNamedElement)?.name == name
                    }?.let { return it }
                E.PROPERTY_ACCESSOR -> {}
            }
            if (scope is JuxTypeDeclaration) {
                val members = membersOf(selfType(scope)).filter { (it.element as? JuxNamedElement)?.name == name }
                if (members.isNotEmpty()) {
                    if (argCount != null) {
                        members.firstOrNull {
                            it.element.elementType === E.METHOD_DECLARATION && JuxHierarchy.arity(it.element) == argCount
                        }?.let { return it.element }
                    }
                    return members.first().element
                }
                // A nested type of this declaration.
                PsiTreeUtil.findChildrenOfType(scope, JuxTypeDeclaration::class.java)
                    .firstOrNull { it.name == name && it !== scope }?.let { return it }
            }
            if (scope is PsiFile) break
            child = scope
            scope = scope.parent
        }
        val file = ref.containingFile
        if (file is JuxFile) {
            val topLevel = ArrayList<PsiElement>()
            for (d in file.children) {
                if (d.elementType === E.EXTERN_BLOCK) {
                    d.children.filterTo(topLevel) { (it as? JuxNamedElement)?.name == name }
                } else if ((d as? JuxNamedElement)?.name == name) {
                    topLevel.add(d)
                }
            }
            if (topLevel.isNotEmpty()) {
                if (argCount != null) {
                    topLevel.firstOrNull {
                        it.elementType === E.METHOD_DECLARATION && JuxHierarchy.arity(it) == argCount
                    }?.let { return it }
                }
                return topLevel.first()
            }
        }
        return resolveTypeName(ref, name)
    }

    /**
     * What `Type::new` refers to (§M.8): the type's constructor when it
     * declares exactly one, else the type itself (overloads are picked by the
     * expected function type, which is the compiler's job).
     */
    fun constructorReferenceTarget(ref: PsiElement): PsiElement? {
        val qualifier = firstExpressionChild(ref) ?: ref.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return null
        val decl = when (val t = if (qualifier.elementType === E.TYPE_REFERENCE) typeOfTypeReference(qualifier) else typeOf(qualifier)) {
            is JuxType.Static -> t.decl
            is JuxType.ClassType -> t.decl
            else -> null
        } ?: return null
        val ctors = PsiTreeUtil.findChildrenOfType(decl, JuxNamedElement::class.java)
            .filter { it.elementType === E.CONSTRUCTOR_DECLARATION && PsiTreeUtil.getParentOfType(it, JuxTypeDeclaration::class.java) === decl }
        return ctors.singleOrNull() ?: decl
    }

    // ------------------------------------------------------------ helpers

    fun isExpression(e: PsiElement): Boolean {
        val t = e.elementType ?: return false
        return t is dev.jux.intellij.psi.JuxElementType && t.toString().endsWith("_EXPRESSION")
    }

    fun expressionChildren(e: PsiElement): List<PsiElement> = e.children.filter { isExpression(it) }

    fun firstExpressionChild(e: PsiElement): PsiElement? {
        var c = e.firstChild
        while (c != null) {
            if (isExpression(c)) return c
            c = c.nextSibling
        }
        return null
    }

    fun argumentCount(call: PsiElement): Int {
        val args = call.node.findChildByType(E.ARGUMENT_LIST)?.psi ?: return 0
        return args.children.count { isExpression(it) }
    }
}
