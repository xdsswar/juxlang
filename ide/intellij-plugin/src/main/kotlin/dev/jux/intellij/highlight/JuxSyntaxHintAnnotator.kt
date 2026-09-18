package dev.jux.intellij.highlight

import com.intellij.codeInsight.intention.IntentionAction
import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.Annotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Mistakes that parse but are almost never meant, with the fix attached.
 *
 * - An unterminated string, char or block comment: the lexer reads to the end
 *   of the line or file, and nothing said so.
 * - `'abc'`: a char literal holds one character.
 * - `if (x = 5)`: assignment where a comparison was meant.
 * - `x => x + 1`: a lambda written with the type-test arrow.
 * - `let x = 3;`, `elif (...)`, `function f()`: another language's keyword.
 */
class JuxSyntaxHintAnnotator : Annotator {

    override fun annotate(element: PsiElement, holder: AnnotationHolder) {
        when (element.elementType) {
            JuxTokenTypes.STRING_LITERAL -> checkQuoted(element, '"', holder)
            JuxTokenTypes.CHAR_LITERAL -> checkChar(element, holder)
            JuxTokenTypes.BLOCK_COMMENT, JuxTokenTypes.DOC_COMMENT ->
                if (!element.text.endsWith("*/") || element.textLength < 4) {
                    holder.newAnnotation(HighlightSeverity.ERROR, "Unclosed comment")
                        .range(element.textRange.endOffset.let { com.intellij.openapi.util.TextRange(it, it) })
                        .afterEndOfLine()
                        .create()
                }
            E.IF_STATEMENT, E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT -> checkConditionAssignment(element, holder)
            E.BINARY_EXPRESSION -> checkFatArrowLambda(element, holder)
            E.LOCAL_VARIABLE -> checkForeignLocalKeyword(element, holder)
            E.CALL_EXPRESSION -> checkElif(element, holder)
            E.METHOD_DECLARATION -> checkFunctionKeyword(element, holder)
        }
    }

    private fun checkQuoted(element: PsiElement, quote: Char, holder: AnnotationHolder) {
        val text = element.text
        if (text.length < 2 || text.last() != quote || text.endsWith("\\$quote") && !text.endsWith("\\\\$quote")) {
            holder.newAnnotation(HighlightSeverity.ERROR, "Illegal line end in string literal")
                .range(element)
                .withFix(ReplaceFix(element.textRange.endOffset, element.textRange.endOffset, quote.toString(), "Close the string"))
                .create()
        }
    }

    private fun checkChar(element: PsiElement, holder: AnnotationHolder) {
        val text = element.text
        if (text.length < 2 || text.last() != '\'') {
            holder.newAnnotation(HighlightSeverity.ERROR, "Unclosed character literal").range(element).create()
            return
        }
        val body = text.substring(1, text.length - 1)
        val chars = if (body.startsWith("\\")) 1 else body.codePointCount(0, body.length)
        if (chars > 1) {
            val range = element.textRange
            holder.newAnnotation(HighlightSeverity.ERROR, "Too many characters in character literal")
                .range(element)
                .withFix(ReplaceFix(range.startOffset, range.endOffset, "\"" + body + "\"", "Convert to a String literal"))
                .create()
        } else if (chars == 0) {
            holder.newAnnotation(HighlightSeverity.ERROR, "Empty character literal").range(element).create()
        }
    }

    /** `if (x = 5)`: the condition itself is an `=` assignment. */
    private fun checkConditionAssignment(statement: PsiElement, holder: AnnotationHolder) {
        val condition = statement.children.firstOrNull { it.elementType === E.ASSIGNMENT_EXPRESSION } ?: return
        val op = condition.node.findChildByType(JuxTokenTypes.EQ) ?: return
        holder.newAnnotation(HighlightSeverity.ERROR, "Assignment in condition; did you mean '=='?")
            .range(op.psi)
            .withFix(ReplaceFix(op.startOffset, op.startOffset + 1, "==", "Replace '=' with '=='"))
            .create()
    }

    /**
     * `list.map(x => x * 2)`: `=>` tests a type, and a lowercase name after it
     * is never a type in idiomatic Jux, so this is a lambda with the wrong arrow.
     */
    private fun checkFatArrowLambda(binary: PsiElement, holder: AnnotationHolder) {
        val arrow = binary.node.findChildByType(JuxTokenTypes.FAT_ARROW) ?: return
        val left = binary.firstChild ?: return
        if (left.elementType !== E.REFERENCE_EXPRESSION && left.elementType !== E.PARENTHESIZED_EXPRESSION) return
        val typeAfter = binary.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim() ?: return
        // A lowercase built-in type (`int`, `any`) is a real type test.
        if (typeAfter.firstOrNull()?.isLowerCase() != true || typeAfter in JuxKeywords.PRIMITIVES ||
            typeAfter in JuxKeywords.BUILTINS
        ) return
        holder.newAnnotation(HighlightSeverity.ERROR, "Lambdas use '->'; '=>' is the type-test operator")
            .range(arrow.psi)
            .withFix(ReplaceFix(arrow.startOffset, arrow.startOffset + 2, "->", "Replace '=>' with '->'"))
            .create()
    }

    /** `let x = 3;` / `const`-less JavaScript habits: the "type" is a keyword from another language. */
    private fun checkForeignLocalKeyword(local: PsiElement, holder: AnnotationHolder) {
        val type = local.node.findChildByType(E.TYPE_REFERENCE) ?: return
        if (type.text.trim() != "let") return
        holder.newAnnotation(HighlightSeverity.ERROR, "Jux declares a local with 'var', not 'let'")
            .range(type.psi)
            .withFix(ReplaceFix(type.startOffset, type.startOffset + 3, "var", "Replace 'let' with 'var'"))
            .create()
    }

    /** `elif (x) { … }`: a call to `elif`. */
    private fun checkElif(call: PsiElement, holder: AnnotationHolder) {
        val callee = call.firstChild ?: return
        if (callee.elementType !== E.REFERENCE_EXPRESSION || callee.text != "elif") return
        holder.newAnnotation(HighlightSeverity.ERROR, "Jux writes 'else if', not 'elif'")
            .range(callee)
            .withFix(ReplaceFix(callee.textRange.startOffset, callee.textRange.endOffset, "else if", "Replace 'elif' with 'else if'"))
            .create()
    }

    /** `function add(a, b)` / `def add(…)` / `func add(…)`: the return type is another language's keyword. */
    private fun checkFunctionKeyword(method: PsiElement, holder: AnnotationHolder) {
        val type = method.node.findChildByType(E.TYPE_REFERENCE) ?: return
        val word = type.text.trim()
        if (word !in FOREIGN_FUNCTION_KEYWORDS) return
        holder.newAnnotation(
            HighlightSeverity.ERROR,
            "Jux has no '$word' keyword; a method starts with its return type, e.g. 'void name(...)' or 'int name(...)'",
        )
            .range(type.psi)
            .withFix(ReplaceFix(type.startOffset, type.startOffset + word.length, "void", "Replace '$word' with 'void'"))
            .create()
    }

    private class ReplaceFix(
        private val start: Int,
        private val end: Int,
        private val replacement: String,
        private val label: String,
    ) : IntentionAction {
        override fun getText(): String = label
        override fun getFamilyName(): String = "Fix syntax"
        override fun isAvailable(project: Project, editor: Editor?, file: PsiFile?): Boolean =
            editor != null && end <= editor.document.textLength
        override fun invoke(project: Project, editor: Editor?, file: PsiFile?) {
            editor?.document?.replaceString(start, end, replacement)
        }
        override fun startInWriteAction(): Boolean = true
    }

    private companion object {
        val FOREIGN_FUNCTION_KEYWORDS = setOf("function", "def", "func", "fun", "fn", "sub")
    }
}
