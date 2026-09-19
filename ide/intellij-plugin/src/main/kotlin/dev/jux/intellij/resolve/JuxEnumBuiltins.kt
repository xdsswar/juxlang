package dev.jux.intellij.resolve

import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * The helpers every enum gets without declaring them (JUX-LANG-V1 §7.7.3,
 * ERRATA E32): `value.name()` and `value.ordinal()` on a variant, and
 * `values()`, `fromName`, `fromNameStrict`, `fromOrdinal` and `cases()` on the
 * type. They have no declaration to navigate to, so completion and the type
 * engine learn them from this table.
 */
object JuxEnumBuiltins {

    /**
     * One helper: its [name], its parameter list as written in the popup, and
     * whether it is called on the type ([isStatic]) or on a variant.
     */
    data class Builtin(val name: String, val params: String, val isStatic: Boolean, val returns: String)

    private val INSTANCE = listOf(
        Builtin("name", "()", isStatic = false, returns = "String"),
        Builtin("ordinal", "()", isStatic = false, returns = "int"),
    )

    /** Only for enums whose variants carry no payload: a payload cannot be invented. */
    private val NO_PAYLOAD_STATIC = listOf(
        Builtin("values", "()", isStatic = true, returns = "Self[]"),
        Builtin("fromName", "(String name)", isStatic = true, returns = "Self?"),
        Builtin("fromNameStrict", "(String name)", isStatic = true, returns = "Self?"),
        Builtin("fromOrdinal", "(int ordinal)", isStatic = true, returns = "Self?"),
    )

    private val CASES = Builtin("cases", "()", isStatic = true, returns = "Vec<EnumCase<Self>>")

    /** The helpers [enum] offers on the type ([static]) or on one of its values. */
    fun of(enum: JuxTypeDeclaration, static: Boolean): List<Builtin> {
        if (enum.elementType !== E.ENUM_DECLARATION) return emptyList()
        if (!static) return INSTANCE
        val out = ArrayList<Builtin>()
        if (!hasPayloadVariant(enum)) out += NO_PAYLOAD_STATIC
        // `cases()` needs a concrete `EnumCase<Self>`: no type parameters.
        if (enum.node.findChildByType(E.TYPE_PARAMETER_LIST) == null) out += CASES
        return out
    }

    /** The helper [name] of [enum], or null when there is none of that name. */
    fun find(enum: JuxTypeDeclaration, name: String, static: Boolean): Builtin? = of(enum, static).firstOrNull { it.name == name }

    /**
     * What calling [builtin] on [enum] gives, as far as the type engine models
     * it: the variant itself for the lookups, `String` / `int` for the
     * instance helpers. `cases()` is left unknown, as `EnumCase` is a
     * library type the engine reaches through its stub.
     */
    fun returnType(enum: JuxTypeDeclaration, builtin: Builtin, context: PsiElement): JuxType {
        val self = JuxType.ClassType(enum, emptyList())
        return when (builtin.returns) {
            "Self?" -> JuxType.Nullable(self)
            "Self[]" -> JuxType.ArrayType(self)
            "int" -> JuxType.Primitive("int")
            "String" -> JuxTypeIndex.findType(context, "String")?.let { JuxType.ClassType(it, emptyList()) }
                ?: JuxType.Primitive("String")
            else -> JuxType.Unknown
        }
    }

    /**
     * Whether a variant of [enum] carries a payload, `Ok(int status)`. A
     * Java-style enum that declares a constructor passes arguments in the
     * same parentheses (`Earth(5.976e+24, 6.37814e6)`), which is no payload.
     */
    private fun hasPayloadVariant(enum: JuxTypeDeclaration): Boolean {
        val body = enum.node.findChildByType(E.CLASS_BODY) ?: return false
        val constants = body.getChildren(null).filter { it.elementType === E.ENUM_CONSTANT }
        if (constants.none { it.findChildByType(T.LPAREN) != null }) return false
        val declaresConstructor = body.getChildren(null).any { it.elementType === E.CONSTRUCTOR_DECLARATION }
        return !declaresConstructor
    }
}
