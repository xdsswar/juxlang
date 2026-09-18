package dev.jux.intellij.templates

import com.intellij.codeInsight.template.TemplateActionContext
import com.intellij.codeInsight.template.TemplateContextType
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiWhiteSpace
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile

/**
 * The live-template contexts for Jux, shaped like Java's
 * (`JavaCodeContextType`): one generic context and, under it, the positions a
 * template can be written for.
 *
 * - **Jux** (`JUX`): anywhere in Jux code except a comment or a string.
 * - **Statement** (`JUX_STATEMENT`): where a statement starts, in a body.
 *   `sout`, `fori`, `iter`, `ifn`, ...
 * - **Expression** (`JUX_EXPRESSION`): where an expression can be written.
 *   `lst`, ...
 * - **Declaration** (`JUX_DECLARATION`): outside every body, where a type or
 *   member is declared. `psvm` here writes a free `void main()` at the top of
 *   a file and `public static void main()` in a type (§E.1.2), through
 *   [JuxMainSignatureMacro].
 *
 * The ids are the option names `liveTemplates/Jux.xml` references; `JUX`
 * stays the generic one so templates a user wrote against it keep working.
 */
open class JuxTemplateContextType(presentableName: String = "Jux") : TemplateContextType(presentableName) {
    override fun isInContext(context: TemplateActionContext): Boolean {
        if (context.file !is JuxFile) return false
        val element = context.file.findElementAt(context.startOffset)
            ?: return isInContextAtEnd(context)
        // Don't expand templates inside comments or string/char literals.
        val type = element.elementType
        if (type in JuxTokenTypes.COMMENTS || type in JuxTokenTypes.STRING_LITERALS) return false
        return isInContext(element)
    }

    /** Whether code at [element] (never a comment or string) is in this context. */
    protected open fun isInContext(element: PsiElement): Boolean = true

    /** The caret at the very end of the file, past every token. */
    protected open fun isInContextAtEnd(context: TemplateActionContext): Boolean = true

    /** A statement starts here, inside a body: `sout`, `fori`, `iter`, ... */
    class Statement : JuxTemplateContextType("Statement") {
        override fun isInContext(element: PsiElement): Boolean = isStatementStart(element)
        override fun isInContextAtEnd(context: TemplateActionContext) = false
    }

    /** An expression can be written here: `lst`, ... */
    class Expression : JuxTemplateContextType("Expression") {
        override fun isInContext(element: PsiElement): Boolean = isExpressionPosition(element)
        override fun isInContextAtEnd(context: TemplateActionContext) = false
    }

    /** A type or member is declared here, outside every body. */
    class Declaration : JuxTemplateContextType("Declaration") {
        override fun isInContext(element: PsiElement): Boolean = isDeclarationPosition(element)
    }

    companion object {
        /**
         * True when [element] is the first token of a statement in a body: the
         * nearest statement-like ancestor sits directly in a code block and
         * starts exactly here. `a.sout` and `x + sout` are not.
         */
        fun isStatementStart(element: PsiElement): Boolean {
            if (isAfterDot(element)) return false
            var node: PsiElement = element
            while (true) {
                val parent = node.parent ?: return false
                if (parent is JuxFile) return false
                if (parent.elementType === E.CODE_BLOCK) {
                    return node.textRange.startOffset == element.textRange.startOffset
                }
                // A declaration or a type body means we left every block.
                if (parent.elementType === E.CLASS_BODY) return false
                node = parent
            }
        }

        /**
         * True when [element] is a bare name in an expression: a reference
         * that nothing qualifies, inside a body or an initializer.
         */
        fun isExpressionPosition(element: PsiElement): Boolean {
            if (element.elementType !== JuxTokenTypes.IDENTIFIER) return false
            if (element.parent?.elementType !== E.REFERENCE_EXPRESSION) return false
            if (isAfterDot(element)) return false
            return PsiTreeUtil.getParentOfType(element, JuxFile::class.java) != null
        }

        /** Outside every code block, and not in an expression. */
        fun isDeclarationPosition(element: PsiElement): Boolean {
            if (isAfterDot(element)) return false
            // A lone word at the top of a file parses as a file-level
            // expression statement; it is where a declaration is being typed.
            val ref = element.parent
            val statement = ref?.parent
            if (ref?.elementType === E.REFERENCE_EXPRESSION &&
                statement?.elementType === E.EXPRESSION_STATEMENT &&
                statement.parent is JuxFile &&
                statement.textRange.startOffset == element.textRange.startOffset
            ) return true
            var p: PsiElement? = element.parent
            while (p != null && p !is JuxFile) {
                val t = p.elementType
                if (t === E.CODE_BLOCK || t === E.PARAMETER_LIST || t === E.ARGUMENT_LIST) return false
                // A field initializer is an expression.
                if (t === E.REFERENCE_EXPRESSION || t === E.CALL_EXPRESSION) return false
                if (t === E.CLASS_BODY) return true
                p = p.parent
            }
            return true
        }

        /** The type body [element] is directly in, if any. */
        fun enclosingTypeBody(element: PsiElement): PsiElement? {
            var p: PsiElement? = element.parent
            while (p != null && p !is JuxFile) {
                if (p.elementType === E.CODE_BLOCK) return null
                if (p.elementType === E.CLASS_BODY) return p
                p = p.parent
            }
            return null
        }

        private fun isAfterDot(element: PsiElement): Boolean {
            var prev = PsiTreeUtil.prevLeaf(element)
            while (prev is PsiWhiteSpace) prev = PsiTreeUtil.prevLeaf(prev)
            val t = prev?.elementType
            return t === JuxTokenTypes.DOT || t === JuxTokenTypes.QUESTION_DOT || t === JuxTokenTypes.COLON_COLON
        }
    }
}
