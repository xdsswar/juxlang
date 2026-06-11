package dev.jux.intellij.format

import com.intellij.formatting.FormattingContext
import com.intellij.formatting.FormattingModel
import com.intellij.formatting.FormattingModelBuilder
import com.intellij.formatting.FormattingModelProvider
import com.intellij.formatting.Indent

/**
 * Wires Reformat Code (Ctrl+Alt+L) for Jux: builds one shared
 * [JuxFormatContext] and a [JuxBlock] tree over the file's AST. Registered as
 * `lang.formatter` in plugin.xml — the default formatting service picks it up;
 * if `juxc fmt` ever becomes the source of truth, an external
 * `formattingService` would replace this model wholesale.
 */
class JuxFormattingModelBuilder : FormattingModelBuilder {
    override fun createModel(formattingContext: FormattingContext): FormattingModel {
        val settings = formattingContext.codeStyleSettings
        val ctx = JuxFormatContext(settings)
        val root = JuxBlock(formattingContext.node, Indent.getNoneIndent(), ctx)
        return FormattingModelProvider.createFormattingModelForPsiFile(
            formattingContext.containingFile, root, settings,
        )
    }
}
