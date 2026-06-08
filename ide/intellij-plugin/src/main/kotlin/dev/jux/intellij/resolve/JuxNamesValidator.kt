package dev.jux.intellij.resolve

import com.intellij.lang.refactoring.NamesValidator
import com.intellij.openapi.project.Project
import dev.jux.intellij.highlight.JuxKeywords

/** Validates identifiers for Rename (and rejects reserved words). */
class JuxNamesValidator : NamesValidator {
    override fun isKeyword(name: String, project: Project?): Boolean =
        name in JuxKeywords.KEYWORDS

    override fun isIdentifier(name: String, project: Project?): Boolean {
        if (name.isEmpty() || name in JuxKeywords.KEYWORDS) return false
        if (!(name[0].isLetter() || name[0] == '_')) return false
        return name.all { it.isLetterOrDigit() || it == '_' }
    }
}
