package dev.jux.intellij.templates

import com.intellij.codeInsight.template.TemplateActionContext
import com.intellij.codeInsight.template.TemplateContextType
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxFile

/**
 * The live-template context for Jux. A template (`psvm`, `sout`, `fore`, …) is
 * offered anywhere in a `.jux` file **except** inside a comment or string
 * literal, matching how IntelliJ's Java templates behave. The `contextId` in
 * `plugin.xml` (`JUX`) is the option name each template references in
 * `liveTemplates/Jux.xml`.
 */
class JuxTemplateContextType : TemplateContextType("Jux") {
    override fun isInContext(context: TemplateActionContext): Boolean {
        if (context.file !is JuxFile) return false
        val element = context.file.findElementAt(context.startOffset) ?: return true
        val type = element.elementType
        // Don't expand templates inside comments or string/char literals.
        return type !in JuxTokenTypes.COMMENTS && type !in JuxTokenTypes.STRING_LITERALS
    }
}
