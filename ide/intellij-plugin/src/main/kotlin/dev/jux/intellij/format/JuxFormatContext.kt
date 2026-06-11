package dev.jux.intellij.format

import com.intellij.formatting.SpacingBuilder
import com.intellij.psi.codeStyle.CodeStyleSettings
import com.intellij.psi.codeStyle.CommonCodeStyleSettings
import dev.jux.intellij.JuxLanguage

/**
 * Per-reformat shared state: the settings, the language's common settings, and
 * **one** [SpacingBuilder] built up front. Every [JuxBlock] in the tree shares
 * this instance — rebuilding the SpacingBuilder per block is the classic
 * formatter performance bug.
 */
class JuxFormatContext(val settings: CodeStyleSettings) {
    val common: CommonCodeStyleSettings = settings.getCommonSettings(JuxLanguage)
    val spacingBuilder: SpacingBuilder = JuxSpacingRules.create(settings, common)
}
