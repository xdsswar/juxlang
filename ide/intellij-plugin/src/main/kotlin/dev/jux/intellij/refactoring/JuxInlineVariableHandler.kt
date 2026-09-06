package dev.jux.intellij.refactoring

import com.intellij.lang.Language
import com.intellij.lang.refactoring.InlineActionHandler
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.codeStyle.CodeStyleManager
import com.intellij.psi.search.LocalSearchScope
import com.intellij.psi.search.searches.ReferencesSearch
import com.intellij.refactoring.util.CommonRefactoringUtil
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxLocalVariable

/**
 * Inline Variable (`Ctrl+Alt+N`) — replace every use of a local with its
 * initializer and delete the declaration.
 *
 * Refused, with a reason, in the three cases where inlining would change the
 * program rather than tidy it:
 *
 * - **the variable is reassigned** — later uses would see the wrong value;
 * - **the initializer has side effects** — one evaluation would become N;
 * - **there is no initializer** — there is nothing to inline.
 *
 * The initializer is parenthesised at each use site unless it is already atomic,
 * because `var d = a + b; … d * 2` must not become `a + b * 2`. Wrapping when in
 * doubt costs a pair of brackets the formatter leaves alone; not wrapping costs
 * a silent arithmetic change.
 */
class JuxInlineVariableHandler : InlineActionHandler() {

    override fun isEnabledForLanguage(l: Language): Boolean = l === JuxLanguage

    override fun canInlineElement(element: PsiElement): Boolean = element is JuxLocalVariable

    override fun getActionName(element: PsiElement?): String = "Inline Variable"

    override fun inlineElement(project: Project, editor: Editor?, element: PsiElement) {
        val local = element as? JuxLocalVariable ?: return
        val name = local.name ?: return

        val initializer = initializerOf(local)
        if (initializer == null) {
            error(project, editor, "`$name` has no initializer, so there is nothing to inline.")
            return
        }
        if (JuxExtractSupport.hasSideEffects(initializer)) {
            error(project, editor, "The initializer of `$name` has side effects. Inlining it would run it once per use instead of once.")
            return
        }

        val scope = JuxExtractSupport.enclosingBlock(local)
        if (scope == null) {
            error(project, editor, "Cannot find the scope `$name` is declared in.")
            return
        }
        if (isReassigned(local, scope, name)) {
            error(project, editor, "`$name` is reassigned, so its uses do not all hold the initializer's value.")
            return
        }

        val usages = ReferencesSearch.search(local, LocalSearchScope(scope))
            .findAll()
            .map { it.element }
            .filter { it.textRange.startOffset != local.nameIdentifier?.textRange?.startOffset }
            .sortedBy { it.textRange.startOffset }
        if (usages.isEmpty()) {
            error(project, editor, "`$name` is never used, so there is nothing to inline. Delete the declaration instead.")
            return
        }

        val replacement = substitutionTextFor(initializer)
        val file = local.containingFile
        val document = PsiDocumentManager.getInstance(project).getDocument(file) ?: return
        val declarationRange = declarationRangeOf(local, document)

        WriteCommandAction.writeCommandAction(project, file)
            .withName("Inline Variable")
            .run<RuntimeException> {
                // Uses first, back to front, then the declaration — which sits
                // before them all, so removing it last keeps every offset valid.
                for (usage in usages.asReversed()) {
                    val range = usage.textRange
                    document.replaceString(range.startOffset, range.endOffset, replacement)
                }
                document.deleteString(declarationRange.first, declarationRange.second)

                val manager = PsiDocumentManager.getInstance(project)
                manager.commitDocument(document)
                CodeStyleManager.getInstance(project).reformatText(
                    file,
                    declarationRange.first,
                    (declarationRange.first + 1).coerceAtMost(document.textLength),
                )
                manager.commitDocument(document)
            }
    }

    /** The initializer expression of a `var x = <expr>;` declaration. */
    private fun initializerOf(local: JuxLocalVariable): PsiElement? {
        val eq = local.node.findChildByType(JuxTokenTypes.EQ) ?: return null
        var node = eq.treeNext
        while (node != null) {
            if (node.elementType in JuxExtractSupport.EXPRESSION_KINDS) return node.psi
            node = node.treeNext
        }
        return null
    }

    /**
     * Whether [name] is written to anywhere in [scope] after its declaration.
     *
     * Assignment targets are not references in this PSI (the left side of `=`
     * is a plain identifier), so this looks for an ASSIGNMENT_EXPRESSION whose
     * first child is exactly the name rather than asking the reference search.
     */
    private fun isReassigned(local: JuxLocalVariable, scope: PsiElement, name: String): Boolean {
        var found = false
        scope.accept(object : com.intellij.psi.PsiRecursiveElementWalkingVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.node?.elementType === E.ASSIGNMENT_EXPRESSION &&
                    element.firstChild?.text?.trim() == name
                ) {
                    found = true
                }
                if (!found) super.visitElement(element)
            }
        })
        // A second `var name = …` in the same block is a redeclaration, which
        // is the same hazard by another spelling.
        return found || scope.children.any { it !== local && it is JuxLocalVariable && it.name == name }
    }

    /**
     * The initializer as it must appear at a use site: parenthesised unless it
     * already binds tighter than anything it could land next to.
     */
    private fun substitutionTextFor(initializer: PsiElement): String {
        val text = initializer.text.trim()
        return if (initializer.node?.elementType in ATOMIC_KINDS) text else "($text)"
    }

    /**
     * The document range of the whole declaration statement, including its
     * trailing newline so inlining does not leave a blank line behind.
     */
    private fun declarationRangeOf(
        local: JuxLocalVariable,
        document: com.intellij.openapi.editor.Document,
    ): Pair<Int, Int> {
        val range = local.textRange
        val line = document.getLineNumber(range.startOffset)
        val lineStart = document.getLineStartOffset(line)
        // Only swallow the leading indentation when the declaration is alone on
        // its line; otherwise the code before it on that line would go too.
        val blankBefore = document.getText(
            com.intellij.openapi.util.TextRange(lineStart, range.startOffset),
        ).isBlank()
        val start = if (blankBefore) lineStart else range.startOffset
        val end = if (blankBefore && line + 1 < document.lineCount) {
            document.getLineStartOffset(line + 1)
        } else {
            range.endOffset
        }
        return start to end
    }

    private fun error(project: Project, editor: Editor?, message: String) {
        CommonRefactoringUtil.showErrorHint(project, editor, message, "Inline Variable", null)
    }

    private companion object {
        /** Expression kinds that never need parentheses at a use site. */
        val ATOMIC_KINDS = setOf(
            E.LITERAL_EXPRESSION,
            E.REFERENCE_EXPRESSION,
            E.PARENTHESIZED_EXPRESSION,
            E.CALL_EXPRESSION,
            E.INDEX_EXPRESSION,
            E.FIELD_ACCESS_EXPRESSION,
            E.NEW_EXPRESSION,
            E.THIS_EXPRESSION,
            E.SUPER_EXPRESSION,
        )
    }
}
