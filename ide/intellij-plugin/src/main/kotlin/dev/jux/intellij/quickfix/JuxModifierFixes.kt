package dev.jux.intellij.quickfix

import com.intellij.codeInspection.LocalQuickFixAndIntentionActionOnPsiElement
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.inspections.JuxCodeFacts
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * Adding and removing one modifier keyword on a declaration, the way Java's
 * `ModifierFix` does: the modifier list is rebuilt from its words so the
 * result keeps Jux's usual order (visibility first).
 */
object JuxModifiers {
    private val VISIBILITY = setOf("public", "private", "protected", "internal")

    /** Adds [keyword] after any visibility word, dropping words in [conflicts]. */
    fun add(project: Project, decl: PsiElement, keyword: String, conflicts: Set<String> = emptySet()) {
        val list = decl.node.findChildByType(E.MODIFIER_LIST)?.psi
        val words = list?.text?.split(Regex("\\s+"))?.filter { it.isNotEmpty() && it !in conflicts }.orEmpty()
        if (keyword in words) return
        val insertAt = words.indexOfLast { it in VISIBILITY } + 1
        val updated = words.toMutableList().apply { add(insertAt, keyword) }.joinToString(" ")
        val fresh = JuxElementFactory.createModifierList(project, updated)
        if (list != null) {
            list.replace(fresh)
            return
        }
        // No modifiers yet: the word goes before the first keyword or type of the header.
        val first = decl.node.getChildren(null).firstOrNull {
            it.elementType !== E.ANNOTATION && it.psi !is com.intellij.psi.PsiWhiteSpace &&
                it.psi !is com.intellij.psi.PsiComment
        }?.psi ?: return
        val at = first.textRange.startOffset
        JuxCodeFacts.edit(project, decl.containingFile, at, at, "$keyword ")
    }

    /** Removes [keyword] from the modifier list (the list itself when it empties). */
    fun remove(project: Project, decl: PsiElement, keyword: String) {
        val list = decl.node.findChildByType(E.MODIFIER_LIST)?.psi ?: return
        val words = list.text.split(Regex("\\s+")).filter { it.isNotEmpty() && it != keyword }
        if (words.isEmpty()) {
            val next = list.nextSibling
            list.delete()
            if (next is com.intellij.psi.PsiWhiteSpace && next.isValid) next.delete()
        } else {
            list.replace(JuxElementFactory.createModifierList(project, words.joinToString(" ")))
        }
    }
}

/** "Make 'Name' abstract", Java's fix for a class that owes abstract methods or declares one. */
class JuxMakeClassAbstractFix(type: JuxTypeDeclaration) : LocalQuickFixAndIntentionActionOnPsiElement(type) {
    private val name = type.name ?: "class"

    override fun getText(): String = "Make '$name' abstract"

    override fun getFamilyName(): String = "Make class abstract"

    override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
        // `final` and `abstract` cannot go together.
        JuxModifiers.add(project, startElement, "abstract", conflicts = setOf("final"))
    }
}

/**
 * "Make 'method' not abstract": drops `abstract` and gives the method a body
 * that throws `UnsupportedOperationException` until it is written.
 */
class JuxMakeMethodNotAbstractFix(method: PsiElement, private val name: String) :
    LocalQuickFixAndIntentionActionOnPsiElement(method) {

    override fun getText(): String = "Make '$name' not abstract"

    override fun getFamilyName(): String = "Make method not abstract"

    override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
        JuxModifiers.remove(project, startElement, "abstract")
        if (JuxHierarchy.hasBody(startElement)) return
        // `;` becomes a body that throws until it is written.
        val semicolon = startElement.node.findChildByType(T.SEMICOLON)?.psi ?: return
        JuxCodeFacts.edit(
            project, file, semicolon.textRange.startOffset, semicolon.textRange.endOffset,
            " {\n${JuxCreateFromUsage.NOT_IMPLEMENTED_BODY}\n}",
        )
    }
}

/** "Make 'method' return 'void'", offered when a method returns no value anywhere. */
class JuxMakeMethodVoidFix(method: PsiElement, private val name: String) :
    LocalQuickFixAndIntentionActionOnPsiElement(method) {

    override fun getText(): String = "Make '$name' return 'void'"

    override fun getFamilyName(): String = "Make method return void"

    override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
        val ref = startElement.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return
        ref.replace(JuxElementFactory.createTypeReference(project, "void"))
    }

    companion object {
        /** True when no `return` in [method]'s own body carries a value (lambdas excluded). */
        fun returnsNoValue(method: PsiElement): Boolean {
            val body = method.node.findChildByType(E.CODE_BLOCK)?.psi ?: return false
            var valued = false
            fun visit(x: PsiElement) {
                if (valued) return
                if (x.elementType === E.LAMBDA_EXPRESSION || x is JuxTypeDeclaration) return
                if (x.elementType === E.RETURN_STATEMENT &&
                    x.children.any { dev.jux.intellij.resolve.JuxTypeEngine.isExpression(it) }
                ) { valued = true; return }
                var c = x.firstChild
                while (c != null) { visit(c); c = c.nextSibling }
            }
            visit(body)
            return !valued
        }
    }
}
