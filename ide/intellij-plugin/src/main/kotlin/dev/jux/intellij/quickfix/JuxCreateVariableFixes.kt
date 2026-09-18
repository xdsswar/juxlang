package dev.jux.intellij.quickfix

import com.intellij.codeInspection.LocalQuickFixAndIntentionActionOnPsiElement
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.inspections.JuxCodeFacts
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * "Create local variable / field / parameter 'name'" on an unresolved bare
 * name, Java's `CreateLocalFromUsageFix`, `CreateFieldFromUsageFix` and
 * `CreateParameterFromUsageFix`.
 *
 * The type comes from the context ([JuxCreateFromUsage.expectedType]), and
 * the declaration starts from the type's default value where Jux has one
 * (`0`, `false`, `""`, `null` for `T?`); the type is a template stop either
 * way. Assigning to the name (`total = 0;`) turns that statement itself into
 * the declaration, as Java's fix does.
 */
object JuxCreateVariables {

    /** The fixes offered for an unresolved value name [reference]. */
    fun fixesFor(reference: PsiElement, name: String): List<LocalQuickFixAndIntentionActionOnPsiElement> {
        val out = ArrayList<LocalQuickFixAndIntentionActionOnPsiElement>()
        if (statementAnchor(reference) != null) out.add(CreateLocalFix(reference, name))
        if (PsiTreeUtil.getParentOfType(reference, JuxTypeDeclaration::class.java) != null) out.add(CreateFieldFix(reference, name))
        if (enclosingCallable(reference) != null) out.add(CreateParameterFix(reference, name))
        return out
    }

    /** The statement that directly sits in a block and contains [e]. */
    fun statementAnchor(e: PsiElement): PsiElement? {
        var p: PsiElement? = e
        while (p != null && p !is JuxFile) {
            val parent = p.parent ?: return null
            if (parent.elementType === E.CODE_BLOCK && JuxCodeFacts.isStatement(p)) return p
            if (parent.elementType === E.CLASS_BODY) return null
            p = parent
        }
        return null
    }

    /** The method or constructor whose body holds [e] (not a lambda's). */
    fun enclosingCallable(e: PsiElement): PsiElement? {
        var p: PsiElement? = e.parent
        while (p != null && p !is JuxFile) {
            when (p.elementType) {
                E.LAMBDA_EXPRESSION, E.CLASS_BODY -> return null
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION ->
                    return p.takeIf { JuxHierarchy.hasBody(it) }
            }
            p = p.parent
        }
        return null
    }

    /** The declared type to write for [reference], or the placeholder. */
    fun typeFor(reference: PsiElement): String =
        JuxCreateFromUsage.expectedType(reference) ?: JuxCreateFromUsage.UNKNOWN_TYPE

    /** `= <default>` for [type], or nothing when Jux has no default to name. */
    fun initializer(type: String): String = JuxCodeFacts.defaultValue(type)?.let { " = $it" } ?: ""
}

/** Declares the name as a local variable just before the statement that uses it. */
class CreateLocalFix(reference: PsiElement, private val name: String) :
    LocalQuickFixAndIntentionActionOnPsiElement(reference) {

    override fun getText(): String = "Create local variable '$name'"

    override fun getFamilyName(): String = "Create local variable from usage"

    override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
        val anchor = JuxCreateVariables.statementAnchor(startElement) ?: return
        val block = anchor.parent
        // `name = value;`: the assignment becomes the declaration.
        val assignment = startElement.parent
        if (assignment?.elementType === E.ASSIGNMENT_EXPRESSION &&
            JuxCodeFacts.binaryOperator(assignment)?.elementType === T.EQ &&
            JuxTypeEngine.firstExpressionChild(assignment) === startElement &&
            assignment.parent === anchor && anchor.elementType === E.EXPRESSION_STATEMENT
        ) {
            val value = JuxTypeEngine.expressionChildren(assignment).getOrNull(1) ?: return
            val type = JuxCreateFromUsage.typeText(JuxTypeEngine.typeOf(value)) ?: JuxCreateFromUsage.UNKNOWN_TYPE
            val decl = JuxElementFactory.createStatement(project, "$type $name = ${value.text};")
            val placed = anchor.replace(decl)
            JuxCreateFromUsage.runTemplate(project, editor, placed, listOfNotNull(placed.node.findChildByType(E.TYPE_REFERENCE)?.psi))
            return
        }
        val type = JuxCreateVariables.typeFor(startElement)
        // On its own line just above the statement that needs it.
        val at = anchor.textRange.startOffset
        val range = JuxCodeFacts.edit(project, file, at, at, "$type $name${JuxCreateVariables.initializer(type)};\n")
        var placed = JuxCodeFacts.elementIn(file, range) ?: return
        while (placed.parent != null && placed.parent !== block && placed.parent.elementType !== E.CODE_BLOCK) placed = placed.parent
        JuxCreateFromUsage.runTemplate(project, editor, placed, listOfNotNull(placed.node.findChildByType(E.TYPE_REFERENCE)?.psi))
    }
}

/** Declares the name as a private field of the enclosing type (static in a static context). */
class CreateFieldFix(reference: PsiElement, private val name: String) :
    LocalQuickFixAndIntentionActionOnPsiElement(reference) {

    override fun getText(): String = "Create field '$name'"

    override fun getFamilyName(): String = "Create field from usage"

    override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
        val type = PsiTreeUtil.getParentOfType(startElement, JuxTypeDeclaration::class.java) ?: return
        val fieldType = JuxCreateVariables.typeFor(startElement)
        val static = if (JuxCreateFromUsage.isInStaticContext(startElement)) " static" else ""
        val text = "private$static $fieldType $name${JuxCreateVariables.initializer(fieldType)};"
        // After the last field, or first in the body when there is none.
        val body = type.node.findChildByType(E.CLASS_BODY)?.psi ?: return
        val anchor = body.children.lastOrNull { it.elementType === E.FIELD_DECLARATION }
            ?: body.node.findChildByType(T.LBRACE)?.psi ?: return
        val next = anchor.nextSibling
        val lineFollows = next is PsiWhiteSpace && next.text.contains('\n')
        val at = anchor.textRange.endOffset
        val range = JuxCodeFacts.edit(project, file, at, at, "\n$text" + if (lineFollows) "" else "\n")
        var placed = JuxCodeFacts.elementIn(file, range) ?: return
        while (placed.parent != null && placed.parent.elementType !== E.CLASS_BODY) placed = placed.parent
        JuxCreateFromUsage.runTemplate(project, editor, placed, listOfNotNull(placed.node.findChildByType(E.TYPE_REFERENCE)?.psi))
    }
}

/** Adds the name as a new last parameter of the enclosing method or constructor. */
class CreateParameterFix(reference: PsiElement, private val name: String) :
    LocalQuickFixAndIntentionActionOnPsiElement(reference) {

    override fun getText(): String = "Create parameter '$name'"

    override fun getFamilyName(): String = "Create parameter from usage"

    override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
        val callable = JuxCreateVariables.enclosingCallable(startElement) ?: return
        val list = callable.node.findChildByType(E.PARAMETER_LIST)?.psi ?: return
        val close = list.node.findChildByType(T.RPAREN)?.psi ?: return
        val hasParams = list.children.any { it.elementType === E.PARAMETER }
        val text = (if (hasParams) ", " else "") + "${JuxCreateVariables.typeFor(startElement)} $name"
        val at = close.textRange.startOffset
        val range = JuxCodeFacts.edit(project, file, at, at, text)
        // The new parameter is the last one before `)`.
        val placed = file.findElementAt(range.endOffset - 1)
            ?.let { leaf -> PsiTreeUtil.findFirstParent(leaf) { it.elementType === E.PARAMETER } } ?: return
        JuxCreateFromUsage.runTemplate(project, editor, placed, listOfNotNull(placed.node.findChildByType(E.TYPE_REFERENCE)?.psi))
    }
}
