package dev.jux.intellij.editor

import com.intellij.codeInsight.editorActions.smartEnter.SmartEnterProcessor
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.TextRange
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.codeStyle.CodeStyleManager
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile

/**
 * Complete Current Statement (`Ctrl+Shift+Enter`) — finish the line the way you
 * were going to, then put the caret where you would have typed next.
 *
 * Three fixers, applied in order to the same line:
 *
 * 1. **Close what is open.** `print(greet(name` gains its two `)`.
 * 2. **Give a header a body.** `if (x > 0)` becomes `if (x > 0) {` + a blank
 *    indented line + `}`, with the caret on the blank line. Same for a member
 *    signature written inside a class body.
 * 3. **Terminate a statement.** Anything else that is not already terminated
 *    gains its `;` and the caret moves to a fresh line.
 *
 * The line is read as **text**, not PSI, because the whole point of the action
 * is that the statement is not finished yet — mid-edit the tree is a recovery
 * tree, and asking it what the author meant gets a worse answer than reading
 * the characters. PSI is consulted for exactly one thing, in step 2: whether
 * the caret is in a class body (a member signature) or a code block (a call).
 */
class JuxSmartEnterProcessor : SmartEnterProcessor() {

    override fun process(project: Project, editor: Editor, psiFile: PsiFile): Boolean {
        if (psiFile !is JuxFile) return false

        val document = editor.document
        val caret = editor.caretModel.offset
        val lineStart = document.getLineStartOffset(document.getLineNumber(caret))
        // Only the text up to the CARET is the statement being completed. Using
        // the whole line would append past whatever follows on it, which is what
        // happens whenever a statement shares its line with a closing brace.
        val raw = document.getText(TextRange(lineStart, caret))

        val code = withoutTrailingComment(raw).trimEnd()
        if (code.isBlank()) return false
        val insertAt = lineStart + code.length

        val closers = missingClosers(code)
        val closed = code + closers

        return when {
            wantsBlock(closed, psiFile, insertAt) ->
                openBlock(project, editor, psiFile, insertAt, closers)

            wantsSemicolon(closed) ->
                terminate(project, editor, psiFile, insertAt, "$closers;")

            closers.isNotEmpty() ->
                terminate(project, editor, psiFile, insertAt, closers)

            else -> false
        }
    }

    // ---- fixers ------------------------------------------------------------

    /** Append `{ }`, reformat, and leave the caret on a blank line inside. */
    private fun openBlock(
        project: Project,
        editor: Editor,
        file: PsiFile,
        insertAt: Int,
        closers: String,
    ): Boolean {
        val document = editor.document
        // The empty line between the braces is written here rather than left to
        // a follow-up Enter: the caret has to land on it, and an offset chosen
        // before reformatting would be stale by the time it ran.
        val text = "$closers {\n\n}"
        document.insertString(insertAt, text)
        val manager = PsiDocumentManager.getInstance(project)
        manager.commitDocument(document)
        CodeStyleManager.getInstance(project).reformatText(file, insertAt, insertAt + text.length)
        manager.commitDocument(document)

        // Reformatting only moves whitespace, so the first `{` at or after the
        // insertion point is still the one we just wrote.
        val brace = document.text.indexOf('{', insertAt)
        if (brace < 0) return true
        caretToIndentedLine(project, editor, file, document.getLineNumber(brace) + 1)
        return true
    }

    /** Append [text] (a `;`, or just the missing closers) and open a fresh line. */
    private fun terminate(
        project: Project,
        editor: Editor,
        file: PsiFile,
        insertAt: Int,
        text: String,
    ): Boolean {
        val document = editor.document
        document.insertString(insertAt, text)
        val manager = PsiDocumentManager.getInstance(project)
        manager.commitDocument(document)
        CodeStyleManager.getInstance(project).reformatText(file, insertAt, insertAt + text.length)
        manager.commitDocument(document)

        val line = document.getLineNumber(insertAt)
        val lineEnd = document.getLineEndOffset(line)
        document.insertString(lineEnd, "\n")
        manager.commitDocument(document)
        caretToIndentedLine(project, editor, file, line + 1)
        return true
    }

    /**
     * Indent [line] to the style's liking and leave the caret at its end.
     *
     * `adjustLineIndent` is asked rather than the caret simply moved, because
     * an empty line carries no indentation of its own — without this the caret
     * lands in column 0 and the next thing typed starts at the left margin.
     */
    private fun caretToIndentedLine(project: Project, editor: Editor, file: PsiFile, line: Int) {
        val document = editor.document
        if (line >= document.lineCount) return
        CodeStyleManager.getInstance(project).adjustLineIndent(file, document.getLineStartOffset(line))
        PsiDocumentManager.getInstance(project).commitDocument(document)
        editor.caretModel.moveToOffset(document.getLineEndOffset(line))
    }

    // ---- line analysis -----------------------------------------------------

    /**
     * The line with any trailing `//` comment removed — but only a `//` that is
     * not inside a string, so `print("http://x")` keeps its argument.
     */
    private fun withoutTrailingComment(line: String): String {
        var inString = false
        var quote = ' '
        var i = 0
        while (i < line.length - 1) {
            val c = line[i]
            when {
                inString && c == '\\' -> i++
                inString && c == quote -> inString = false
                !inString && (c == '"' || c == '\'') -> {
                    inString = true
                    quote = c
                }

                !inString && c == '/' && line[i + 1] == '/' -> return line.substring(0, i)
            }
            i++
        }
        return line
    }

    /** The closing brackets [code] is missing, innermost first. */
    private fun missingClosers(code: String): String {
        val open = ArrayDeque<Char>()
        var inString = false
        var quote = ' '
        var i = 0
        while (i < code.length) {
            val c = code[i]
            when {
                inString && c == '\\' -> i++
                inString && c == quote -> inString = false
                !inString && (c == '"' || c == '\'') -> {
                    inString = true
                    quote = c
                }

                !inString && (c == '(' || c == '[') -> open.addLast(c)
                !inString && (c == ')' || c == ']') -> open.removeLastOrNull()
            }
            i++
        }
        // Unterminated string first: closing it before the brackets is the only
        // order that produces valid text.
        val stringFix = if (inString) quote.toString() else ""
        return stringFix + open.reversed().joinToString("") { if (it == '(') ")" else "]" }
    }

    /**
     * Whether the completed line is a header that wants a `{ }` body: a
     * control-flow keyword with its parentheses closed, a bodyless `else` /
     * `do` / `try` / `finally`, or a member signature in a class body.
     */
    private fun wantsBlock(code: String, file: PsiFile, offset: Int): Boolean {
        if (code.endsWith("{") || code.endsWith(";") || code.endsWith("}")) return false
        val head = code.trimStart()

        if (BODYLESS_KEYWORDS.any { head == it || head.startsWith("$it ") || head.startsWith("$it{") }) return true
        if (HEADER_KEYWORDS.any { head.startsWith("$it ") || head.startsWith("$it(") }) {
            return code.endsWith(")")
        }
        // A member signature: `public int area()` written directly in a class
        // body wants a body, not a `;`. Inside a code block the same shape is
        // a call statement and must get its semicolon instead.
        return code.endsWith(")") && inClassBodyDirectly(file, offset)
    }

    /** Whether the completed line still needs a `;`. */
    private fun wantsSemicolon(code: String): Boolean {
        val last = code.lastOrNull() ?: return false
        if (last in NEVER_TERMINATED) return false
        val head = code.trimStart()
        if (head.startsWith("@") || head.startsWith("//") || head.startsWith("/*") || head.startsWith("*")) return false
        // A header whose parens are still unbalanced is handled by the block
        // fixer once they close; adding a `;` here would cement the mistake.
        if (HEADER_KEYWORDS.any { head.startsWith("$it ") || head.startsWith("$it(") }) return false
        if (BODYLESS_KEYWORDS.any { head == it }) return false
        return true
    }

    /**
     * Whether [offset] sits directly in a class body rather than inside one of
     * its methods. Walks out from the token before the caret; a null answer
     * (the tree has not caught up) reads as "not in a class body", which keeps
     * the safer semicolon fixer.
     */
    private fun inClassBodyDirectly(file: PsiFile, offset: Int): Boolean {
        var e: PsiElement? = file.findElementAt((offset - 1).coerceAtLeast(0))
        while (e != null) {
            when (e.node?.elementType) {
                E.CODE_BLOCK -> return false
                E.CLASS_BODY -> return true
            }
            e = e.parent
        }
        return false
    }

    private companion object {
        /** Keywords whose body follows a closed `( … )`. */
        val HEADER_KEYWORDS = listOf("if", "while", "for", "switch", "catch", "unsafe")

        /** Keywords that take a body with no parentheses at all. */
        val BODYLESS_KEYWORDS = listOf("else", "do", "try", "finally", "unsafe")

        /** Line endings that already terminate, or that a `;` must not follow. */
        val NEVER_TERMINATED = setOf(';', '{', '}', ',', ':')
    }
}
