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
                    for (kw in JuxKeywords.KEYWORDS) {
                        result.addElement(LookupElementBuilder.create(kw).bold())
                    }
                    val seen = HashSet<String>()
                    PsiTreeUtil.processElements(parameters.originalFile, PsiElementProcessor { e ->
                        if (e is JuxNamedElement) e.name?.let { if (seen.add(it)) result.addElement(LookupElementBuilder.create(it)) }
                        true
                    })
                }
            },
        )
    }
}
