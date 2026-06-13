package dev.jux.intellij.editor

import com.intellij.codeInsight.editorActions.TypedHandlerDelegate
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.fileTypes.FileType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.highlight.JuxTokenTypes

/**
 * Smart typing for interpolation strings — the one case the plain
 * [JuxQuoteHandler] / brace matcher can't cover, because at the moment the
 * opener is typed the lexer hasn't yet seen a complete `$"` / `${` token:
 *
 * - **`$"` opener**: typing `"` right after a `$` auto-inserts the matching
 *   close quote and parks the caret between them, so `print($` + `"` becomes
 *   `print($"<caret>")`. (A lone `$` is not a string token, so the standard
 *   quote handler stays silent — hence this delegate.)
 * - **`${` hole**: typing `{` right after a `$` *inside* an interpolation
 *   literal auto-inserts the closing `}` (`$"… ${<caret>}"`). The platform
 *   suppresses brace pairing inside string literals, so the hole would
 *   otherwise stay open.
 *
 * The `$"` close is done in [beforeCharTyped] — it must run *before* the
 * platform's quote machinery (`TypedQuoteImpl.beforeQuoteTyped`), which, for a
 * `"`, returns early and never reaches the delegates' `charTyped`. The `${`
 * close is done in [charTyped] (a `{` is not a quote, so `charTyped` is
 * reached normally). Both only ever ADD a single closing char at a natural
 * boundary — never when one is already there — so they can't double up with
 * the quote handler or with existing content. Self-guards on [JuxFileType].
 */
class JuxTypedHandler : TypedHandlerDelegate() {
    /**
     * `$"` → `$"<caret>"`. Runs before the `"` is inserted: if the caret
     * directly follows a `$` and sits at a natural close boundary (EOL/EOF,
     * whitespace, or a closing delimiter — never an existing `"`), insert the
     * pair ourselves, park the caret between them, and STOP so the platform's
     * quote path doesn't also type a `"`.
     */
    override fun beforeCharTyped(
        c: Char,
        project: Project,
        editor: Editor,
        file: PsiFile,
        fileType: FileType,
    ): Result {
        if (file.fileType != JuxFileType || c != '"') return Result.CONTINUE
        val doc = editor.document
        val text = doc.charsSequence
        val offset = editor.caretModel.offset
        if (offset < 1 || text[offset - 1] != '$') return Result.CONTINUE
        if (!atClosableBoundary(text, offset)) return Result.CONTINUE
        doc.insertString(offset, "\"\"")
        editor.caretModel.moveToOffset(offset + 1) // between the two quotes
        return Result.STOP
    }

    override fun charTyped(c: Char, project: Project, editor: Editor, file: PsiFile): Result {
        if (file.fileType != JuxFileType) return Result.CONTINUE
        if (c == '{') autoCloseInterpHole(project, editor, file)
        return Result.CONTINUE
    }

    /**
     * `${` → `${<caret>}`, but only INSIDE an interpolation literal — `${` in
     * ordinary code is the platform's job. Commits the document first so the
     * freshly-typed `{` is reflected in the PSI before we ask what token the
     * caret is in (this branch is gated on the cheap `$`/`{` char check, so the
     * commit only happens in the narrow case that matters).
     */
    private fun autoCloseInterpHole(project: Project, editor: Editor, file: PsiFile) {
        val doc = editor.document
        val text = doc.charsSequence
        val offset = editor.caretModel.offset
        if (offset < 2) return
        if (text[offset - 1] != '{' || text[offset - 2] != '$') return
        if (offset < text.length && text[offset] == '}') return // already closed

        PsiDocumentManager.getInstance(project).commitDocument(doc)
        val type = file.findElementAt(offset - 1)?.elementType ?: return
        if (type != JuxTokenTypes.INTERP_STRING_LITERAL &&
            type != JuxTokenTypes.INTERP_RAW_STRING_LITERAL
        ) {
            return
        }
        doc.insertString(offset, "}")
        // Caret remains at `offset`, i.e. inside the hole.
    }

    /** EOF, whitespace, or a closing delimiter — never an existing `"`. */
    private fun atClosableBoundary(text: CharSequence, offset: Int): Boolean {
        if (offset >= text.length) return true
        return when (text[offset]) {
            ')', ']', '}', ',', ';' -> true
            else -> text[offset].isWhitespace()
        }
    }
}
