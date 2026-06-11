package dev.jux.intellij.psi

import com.intellij.psi.tree.IElementType
import dev.jux.intellij.JuxLanguage

/** An interior (composite) PSI node type for a Jux grammar production. */
class JuxElementType(debugName: String) : IElementType(debugName, JuxLanguage)

/**
 * The vocabulary of composite PSI node types the parser produces — one per
 * grammar production, modelled on IntelliJ's `JavaElementType`. Leaf tokens
 * live in [dev.jux.intellij.highlight.JuxTokenTypes]; these are the *interior*
 * nodes that give the tree its shape (and drive Structure View, folding, and
 * navigation).
 *
 * The full statement/expression vocabulary is declared here up front so the
 * parser can be deepened without touching this registry; nodes the parser does
 * not yet emit simply go unused until it does.
 */
object JuxElementTypes {
    // ---- compilation unit -------------------------------------------------
    val PACKAGE_STATEMENT = JuxElementType("PACKAGE_STATEMENT")
    val IMPORT_STATEMENT = JuxElementType("IMPORT_STATEMENT")
    val IMPORT_ITEM = JuxElementType("IMPORT_ITEM")

    // ---- names / references ----------------------------------------------
    val QUALIFIED_NAME = JuxElementType("QUALIFIED_NAME")
    val REFERENCE_EXPRESSION = JuxElementType("REFERENCE_EXPRESSION")
    val TYPE_REFERENCE = JuxElementType("TYPE_REFERENCE")
    val TYPE_ARGUMENT_LIST = JuxElementType("TYPE_ARGUMENT_LIST")
    val WILDCARD_TYPE = JuxElementType("WILDCARD_TYPE")

    // ---- modifiers / annotations -----------------------------------------
    val MODIFIER_LIST = JuxElementType("MODIFIER_LIST")
    val ANNOTATION = JuxElementType("ANNOTATION")
    val ANNOTATION_ARGUMENT_LIST = JuxElementType("ANNOTATION_ARGUMENT_LIST")
    val ANNOTATION_ARGUMENT = JuxElementType("ANNOTATION_ARGUMENT")

    // ---- type declarations -----------------------------------------------
    val CLASS_DECLARATION = JuxElementType("CLASS_DECLARATION")
    val INTERFACE_DECLARATION = JuxElementType("INTERFACE_DECLARATION")
    val ENUM_DECLARATION = JuxElementType("ENUM_DECLARATION")
    val RECORD_DECLARATION = JuxElementType("RECORD_DECLARATION")
    val STRUCT_DECLARATION = JuxElementType("STRUCT_DECLARATION")
    val ANNOTATION_DECLARATION = JuxElementType("ANNOTATION_DECLARATION")
    val TYPE_ALIAS_DECLARATION = JuxElementType("TYPE_ALIAS_DECLARATION")

    val TYPE_PARAMETER_LIST = JuxElementType("TYPE_PARAMETER_LIST")
    val TYPE_PARAMETER = JuxElementType("TYPE_PARAMETER")
    val EXTENDS_CLAUSE = JuxElementType("EXTENDS_CLAUSE")
    val IMPLEMENTS_CLAUSE = JuxElementType("IMPLEMENTS_CLAUSE")
    val PERMITS_CLAUSE = JuxElementType("PERMITS_CLAUSE")
    val RECORD_COMPONENT_LIST = JuxElementType("RECORD_COMPONENT_LIST")
    val RECORD_COMPONENT = JuxElementType("RECORD_COMPONENT")

    // ---- members ----------------------------------------------------------
    val CLASS_BODY = JuxElementType("CLASS_BODY")
    val FIELD_DECLARATION = JuxElementType("FIELD_DECLARATION")
    val METHOD_DECLARATION = JuxElementType("METHOD_DECLARATION")
    val CONSTRUCTOR_DECLARATION = JuxElementType("CONSTRUCTOR_DECLARATION")
    val OPERATOR_DECLARATION = JuxElementType("OPERATOR_DECLARATION")
    val CONST_DECLARATION = JuxElementType("CONST_DECLARATION")
    val PROPERTY_DECLARATION = JuxElementType("PROPERTY_DECLARATION")
    val ENUM_CONSTANT = JuxElementType("ENUM_CONSTANT")
    val INIT_BLOCK = JuxElementType("INIT_BLOCK")
    val STATIC_BLOCK = JuxElementType("STATIC_BLOCK")
    val DROP_BLOCK = JuxElementType("DROP_BLOCK")

    val PARAMETER_LIST = JuxElementType("PARAMETER_LIST")
    val PARAMETER = JuxElementType("PARAMETER")
    val THROWS_CLAUSE = JuxElementType("THROWS_CLAUSE")
    val WHERE_CLAUSE = JuxElementType("WHERE_CLAUSE")

    // ---- statements -------------------------------------------------------
    val CODE_BLOCK = JuxElementType("CODE_BLOCK")
    val LOCAL_VARIABLE = JuxElementType("LOCAL_VARIABLE")
    val EXPRESSION_STATEMENT = JuxElementType("EXPRESSION_STATEMENT")
    val IF_STATEMENT = JuxElementType("IF_STATEMENT")
    val WHILE_STATEMENT = JuxElementType("WHILE_STATEMENT")
    val DO_WHILE_STATEMENT = JuxElementType("DO_WHILE_STATEMENT")
    val FOR_STATEMENT = JuxElementType("FOR_STATEMENT")
    val FOR_EACH_STATEMENT = JuxElementType("FOR_EACH_STATEMENT")
    val SWITCH_STATEMENT = JuxElementType("SWITCH_STATEMENT")
    val SWITCH_CASE = JuxElementType("SWITCH_CASE")
    val RETURN_STATEMENT = JuxElementType("RETURN_STATEMENT")
    val BREAK_STATEMENT = JuxElementType("BREAK_STATEMENT")
    val CONTINUE_STATEMENT = JuxElementType("CONTINUE_STATEMENT")
    val THROW_STATEMENT = JuxElementType("THROW_STATEMENT")
    val TRY_STATEMENT = JuxElementType("TRY_STATEMENT")
    val CATCH_CLAUSE = JuxElementType("CATCH_CLAUSE")
    val FINALLY_CLAUSE = JuxElementType("FINALLY_CLAUSE")
    val UNSAFE_STATEMENT = JuxElementType("UNSAFE_STATEMENT")
    val LABELED_STATEMENT = JuxElementType("LABELED_STATEMENT")
    val EMPTY_STATEMENT = JuxElementType("EMPTY_STATEMENT")

    // ---- expressions ------------------------------------------------------
    val LITERAL_EXPRESSION = JuxElementType("LITERAL_EXPRESSION")
    val PARENTHESIZED_EXPRESSION = JuxElementType("PARENTHESIZED_EXPRESSION")
    val BINARY_EXPRESSION = JuxElementType("BINARY_EXPRESSION")
    val UNARY_EXPRESSION = JuxElementType("UNARY_EXPRESSION")
    val POSTFIX_EXPRESSION = JuxElementType("POSTFIX_EXPRESSION")
    val ASSIGNMENT_EXPRESSION = JuxElementType("ASSIGNMENT_EXPRESSION")
    val CONDITIONAL_EXPRESSION = JuxElementType("CONDITIONAL_EXPRESSION")
    val RANGE_EXPRESSION = JuxElementType("RANGE_EXPRESSION")
    val CALL_EXPRESSION = JuxElementType("CALL_EXPRESSION")
    val ARGUMENT_LIST = JuxElementType("ARGUMENT_LIST")
    val INDEX_EXPRESSION = JuxElementType("INDEX_EXPRESSION")
    val FIELD_ACCESS_EXPRESSION = JuxElementType("FIELD_ACCESS_EXPRESSION")
    val CAST_EXPRESSION = JuxElementType("CAST_EXPRESSION")
    val NEW_EXPRESSION = JuxElementType("NEW_EXPRESSION")
    val LAMBDA_EXPRESSION = JuxElementType("LAMBDA_EXPRESSION")
    val METHOD_REF_EXPRESSION = JuxElementType("METHOD_REF_EXPRESSION")
    val SWITCH_EXPRESSION = JuxElementType("SWITCH_EXPRESSION")
    val THIS_EXPRESSION = JuxElementType("THIS_EXPRESSION")
    val SUPER_EXPRESSION = JuxElementType("SUPER_EXPRESSION")

    // ---- patterns ---------------------------------------------------------
    val PATTERN = JuxElementType("PATTERN")
    val PATTERN_GUARD = JuxElementType("PATTERN_GUARD")
}
