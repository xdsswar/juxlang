package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.CompletionContributor
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionProvider
import com.intellij.codeInsight.completion.CompletionResultSet
import com.intellij.codeInsight.completion.CompletionType
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.patterns.PlatformPatterns
import com.intellij.psi.search.PsiElementProcessor
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.util.ProcessingContext
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.highlight.JuxKeywords
import dev.jux.intellij.psi.JuxNamedElement

/**
 * Basic IDE-side completion: reserved keywords plus the named declarations in
 * the current file. Cross-file and std/library completion continues to come
 * from `juxc-lsp` (Rust std = Jux std); this keeps completion useful on
 * Community IDEs without the LSP.
 */
class JuxCompletionContributor : CompletionContributor() {
    init {
        extend(
            CompletionType.BASIC,
            PlatformPatterns.psiElement().withLanguage(JuxLanguage),
            object : CompletionProvider<CompletionParameters>() {
                override fun addCompletions(
                    parameters: CompletionParameters,
                    context: ProcessingContext,
                    result: CompletionResultSet,
                ) {
                    // After a `.` (member access) the only relevant completions
                    // are the receiver's members, which come from `juxc-lsp`.
                    // Contributing keywords/declarations here would push the
                    // class members down the list (the user has to scroll past
                    // `for`/`if`/… to reach them), so bail out entirely.
                    if (isAfterDot(parameters)) return

                    // Only the keywords the grammar accepts at this position
                    // (statements in a block, members in a class body, …).
                    // `position` is the dummy-identifier leaf in the completion
                    // copy — always present and parsed, even in an empty file.
                    val keywords = JuxKeywordContext.keywordsFor(parameters.position)
                    for (kw in keywords) {
                        result.addElement(LookupElementBuilder.create(kw).bold())
                    }
                    val seen = HashSet<String>()
                    PsiTreeUtil.processElements(parameters.originalFile, PsiElementProcessor { e ->
                        if (e is JuxNamedElement) e.name?.let { if (seen.add(it)) result.addElement(LookupElementBuilder.create(it)) }
                        true
                    })
                }

                /**
                 * True when the caret sits in a member-access position — i.e.
                 * the nearest non-identifier, non-whitespace char before the
                 * (possibly partial) name being completed is a `.`.
                 */
                private fun isAfterDot(parameters: CompletionParameters): Boolean {
                    val text = parameters.editor.document.charsSequence
                    var i = parameters.offset - 1
                    while (i >= 0 && (text[i].isLetterOrDigit() || text[i] == '_')) i--
                    while (i >= 0 && text[i].isWhitespace()) i--
                    return i >= 0 && text[i] == '.'
                }
            },
        )
    }
}
