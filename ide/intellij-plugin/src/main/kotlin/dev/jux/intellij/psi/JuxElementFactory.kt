package dev.jux.intellij.psi

import com.intellij.openapi.project.Project
import com.intellij.psi.PsiComment
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFileFactory
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.highlight.JuxTokenTypes

/**
 * Builds throwaway PSI fragments by parsing a small dummy file and lifting the
 * piece out. Rename mints identifier leaves here; quick fixes mint the
 * statements, expressions and members they splice into the user's file.
 */
object JuxElementFactory {
    /** A standalone identifier leaf with text [name], parsed from a dummy file. */
    fun createIdentifier(project: Project, name: String): PsiElement {
        val file = createFile(project, "class $name {}")
        return PsiTreeUtil.collectElements(file) { it.elementType === JuxTokenTypes.IDENTIFIER }.first()
    }

    /** A whole dummy Jux file holding [text]. */
    fun createFile(project: Project, text: String): JuxFile =
        PsiFileFactory.getInstance(project).createFileFromText("_dummy.jux", JuxFileType, text) as JuxFile

    /** An expression, lifted from the initializer of `var __jux = <text>;`. */
    fun createExpression(project: Project, text: String): PsiElement {
        val file = createFile(project, "void __jux() {\n    var __jux = $text;\n}\n")
        val local = PsiTreeUtil.collectElements(file) { it.elementType === JuxElementTypes.LOCAL_VARIABLE }.first()
        var sawEq = false
        var child = local.firstChild
        while (child != null) {
            if (child.elementType === JuxTokenTypes.EQ) sawEq = true
            else if (sawEq && child !is PsiWhiteSpace && child !is PsiComment) return child
            child = child.nextSibling
        }
        error("no expression parsed from: $text")
    }

    /** One statement, the first inside a dummy function body. */
    fun createStatement(project: Project, text: String): PsiElement =
        createStatements(project, text).first()

    /** Every statement [text] holds, as they parse inside a function body. */
    fun createStatements(project: Project, text: String): List<PsiElement> {
        val file = createFile(project, "void __jux() {\n$text\n}\n")
        val block = PsiTreeUtil.collectElements(file) { it.elementType === JuxElementTypes.CODE_BLOCK }.first()
        return block.children.filter { it.node.elementType !== JuxTokenTypes.LBRACE && it.node.elementType !== JuxTokenTypes.RBRACE }
    }

    /** A code block `{ … }` holding [text]. */
    fun createCodeBlock(project: Project, text: String): PsiElement {
        val file = createFile(project, "void __jux() {\n$text\n}\n")
        return PsiTreeUtil.collectElements(file) { it.elementType === JuxElementTypes.CODE_BLOCK }.first()
    }

    /** One type member (field, method, constructor, nested type), parsed in a dummy class body. */
    fun createMember(project: Project, text: String, className: String = "__Jux"): PsiElement {
        val file = createFile(project, "class $className {\n$text\n}\n")
        val body = PsiTreeUtil.collectElements(file) { it.elementType === JuxElementTypes.CLASS_BODY }.first()
        return body.children.first {
            it !is PsiWhiteSpace && it !is PsiComment &&
                it.elementType !== JuxTokenTypes.LBRACE && it.elementType !== JuxTokenTypes.RBRACE
        }
    }

    /** One top-level declaration (a free function, a type), parsed as a file. */
    fun createTopLevel(project: Project, text: String): PsiElement =
        createFile(project, text).children.first { it !is PsiWhiteSpace && it !is PsiComment }

    /** A parameter `Type name`, lifted from a dummy function's parameter list. */
    fun createParameter(project: Project, text: String): PsiElement {
        val file = createFile(project, "void __jux($text) {}\n")
        return PsiTreeUtil.collectElements(file) { it.elementType === JuxElementTypes.PARAMETER }.first()
    }

    /** A type reference `T`, lifted from a dummy field declaration. */
    fun createTypeReference(project: Project, text: String): PsiElement {
        val file = createFile(project, "class __Jux {\n    $text __jux;\n}\n")
        return PsiTreeUtil.collectElements(file) { it.elementType === JuxElementTypes.TYPE_REFERENCE }.first()
    }

    /** A modifier list holding exactly [text] (`abstract`, `private static`, ...). */
    fun createModifierList(project: Project, text: String): PsiElement {
        val file = createFile(project, "$text class __Jux {}\n")
        return PsiTreeUtil.collectElements(file) { it.elementType === JuxElementTypes.MODIFIER_LIST }.first()
    }

    /** A whitespace leaf holding [text] (newlines, indentation). */
    fun createWhitespace(project: Project, text: String): PsiElement {
        val file = createFile(project, "${text}class __Jux {}")
        return file.firstChild as PsiWhiteSpace
    }
}
