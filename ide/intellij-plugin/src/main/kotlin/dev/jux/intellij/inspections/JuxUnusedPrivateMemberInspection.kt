package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.editor.JuxImportSupport
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxPropertyDeclaration
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * "Unused declaration" for the members only the file itself can use, the
 * part of Java's `UnusedDeclaration` the IDE can decide exactly: `private`
 * is scoped to the top-level declaration's file (JUX-LANG-V1 §6.6), so a
 * `private` method, `static` field or constructor that nothing in the file
 * mentions is dead. Private instance fields, locals and parameters are
 * [JuxUnusedLocalSymbolInspection]'s.
 *
 * A member counts as used when its name appears anywhere else in the file:
 * as any identifier (a call, a read, a method reference, an overload with the
 * same name), or inside an interpolated string. That over-counts (a local
 * with the same name keeps a method alive), and that is the point: the fix
 * deletes code, so it must never fire on a member that is used.
 *
 * Skipped: `@override` methods (the supertype calls them), operators, `main`,
 * and a private constructor with no parameters (the "cannot be instantiated"
 * idiom, which Java skips too).
 */
class JuxUnusedPrivateMemberInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val candidates = ArrayList<PsiElement>()
        PsiTreeUtil.processElements(file) { e ->
            if (e.parent?.elementType === E.CLASS_BODY && JuxHierarchy.hasModifier(e, "private")) {
                when (e.elementType) {
                    E.METHOD_DECLARATION -> if (!isOverride(e) && (e as? JuxNamedElement)?.name != "main") candidates.add(e)
                    E.CONSTRUCTOR_DECLARATION -> if (JuxHierarchy.parameters(e).isNotEmpty()) candidates.add(e)
                    E.FIELD_DECLARATION -> if (e !is JuxPropertyDeclaration && JuxHierarchy.hasModifier(e, "static")) candidates.add(e)
                }
            }
            true
        }
        if (candidates.isEmpty()) return null

        // Every mention of a name in the file, with the leaf that mentions it.
        val mentions = HashMap<String, MutableList<PsiElement>>()
        val interpolated = HashSet<String>()
        PsiTreeUtil.processElements(file) { e ->
            when (e.elementType) {
                T.IDENTIFIER -> mentions.getOrPut(e.text) { ArrayList() }.add(e)
                T.INTERP_STRING_LITERAL -> interpolated.addAll(JuxImportSupport.interpolatedNames(e.text, raw = false))
                T.INTERP_RAW_STRING_LITERAL -> interpolated.addAll(JuxImportSupport.interpolatedNames(e.text, raw = true))
            }
            true
        }

        val problems = ArrayList<ProblemDescriptor>()
        for (member in candidates) {
            val (name, kind) = when (member.elementType) {
                E.CONSTRUCTOR_DECLARATION -> {
                    val type = member.parent?.parent as? JuxTypeDeclaration ?: continue
                    // Enum constants call their constructor with no `new`.
                    if (!JuxHierarchy.isClass(type)) continue
                    val typeName = type.name ?: continue
                    if (constructorUsed(file, type, typeName, member)) continue
                    typeName to "Constructor"
                }
                E.METHOD_DECLARATION -> {
                    val n = (member as JuxNamedElement).name ?: continue
                    if (n in interpolated || mentions[n].orEmpty().any { it !== member.nameIdentifier }) continue
                    n to "Method"
                }
                else -> {
                    val n = (member as JuxNamedElement).name ?: continue
                    if (n in interpolated || mentions[n].orEmpty().any { it !== member.nameIdentifier }) continue
                    n to "Field"
                }
            }
            val anchor = if (kind == "Constructor") member.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: member
            else (member as JuxNamedElement).nameIdentifier ?: member
            val shown = if (kind == "Constructor") "$kind '$name(...)'" else "$kind '$name'"
            problems.add(
                manager.createProblemDescriptor(
                    anchor,
                    "Private ${shown.replaceFirstChar { it.lowercase() }} is never used",
                    isOnTheFly,
                    arrayOf<LocalQuickFix>(SafeDeleteMemberFix(kind.lowercase())),
                    ProblemHighlightType.LIKE_UNUSED_SYMBOL,
                ),
            )
        }
        return problems.toTypedArray()
    }

    private fun isOverride(method: PsiElement): Boolean = method.children.any {
        it.elementType === E.ANNOTATION &&
            it.text.removePrefix("@").substringBefore('(').trim().equals("override", ignoreCase = true)
    }

    /**
     * True when something in [file] may construct through [ctor]: a
     * `new Type(...)` or `Type(...)` call with its number of arguments (any
     * number when it takes varargs), `Type::new`, a `this(...)` from a sibling
     * constructor, or a class extending [type] (its `super(...)`, implicit or
     * not, may land here).
     */
    private fun constructorUsed(file: PsiFile, type: JuxTypeDeclaration, typeName: String, ctor: PsiElement): Boolean {
        val params = JuxHierarchy.parameters(ctor)
        val varargs = params.any { it.node.findChildByType(T.ELLIPSIS) != null }
        val arity = params.size
        var used = false
        PsiTreeUtil.processElements(file) { e ->
            when (e.elementType) {
                E.NEW_EXPRESSION -> {
                    val ref = e.node.findChildByType(E.TYPE_REFERENCE)?.psi
                    if (ref != null && JuxHierarchy.bareTypeName(ref) == typeName &&
                        (varargs || dev.jux.intellij.resolve.JuxTypeEngine.argumentCount(e) == arity)
                    ) used = true
                }
                E.CALL_EXPRESSION -> {
                    val callee = e.firstChild
                    if (callee?.elementType === E.THIS_EXPRESSION && PsiTreeUtil.isAncestor(type, e, true)) used = true
                    if (callee?.elementType === E.REFERENCE_EXPRESSION && callee.text == typeName) used = true
                }
                E.METHOD_REF_EXPRESSION -> if (e.text.replace(" ", "") == "$typeName::new") used = true
                E.EXTENDS_CLAUSE -> if (e.children.any {
                        it.elementType === E.TYPE_REFERENCE && JuxHierarchy.bareTypeName(it) == typeName
                    }) used = true
            }
            !used
        }
        return used
    }

    /** Deletes the member, its doc comment, and the blank line it leaves. */
    private class SafeDeleteMemberFix(private val kind: String) : LocalQuickFix {
        override fun getFamilyName(): String = "Safe delete unused $kind"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            var member: PsiElement? = descriptor.psiElement ?: return
            while (member != null && member.parent?.elementType !== E.CLASS_BODY) member = member.parent
            val decl = member ?: return
            // A doc comment directly above belongs to the member.
            var prev = decl.prevSibling
            if (prev is PsiWhiteSpace && !prev.text.contains("\n\n")) prev = prev.prevSibling
            if (prev is PsiComment && prev.elementType === T.DOC_COMMENT) {
                val gap = prev.nextSibling
                prev.delete()
                if (gap is PsiWhiteSpace && gap.isValid) gap.delete()
            }
            val before = decl.prevSibling
            decl.delete()
            if (before is PsiWhiteSpace && before.isValid && before.text.count { it == '\n' } > 1) {
                before.replace(dev.jux.intellij.psi.JuxElementFactory.createWhitespace(project, "\n" + before.text.substringAfterLast('\n')))
            }
        }
    }
}
