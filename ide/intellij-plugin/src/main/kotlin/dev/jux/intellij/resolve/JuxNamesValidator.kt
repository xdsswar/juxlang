package dev.jux.intellij.resolve

import com.intellij.lang.refactoring.NamesValidator
import com.intellij.openapi.project.Project
import dev.jux.intellij.highlight.JuxKeywords

/**
 * Validates identifiers for Rename (and rejects reserved words).
 *
 * A declaration's name is an identifier position, so every keyword is refused
 * there. A package segment is not always one: see [isPackageSegment].
 */
class JuxNamesValidator : NamesValidator {
    override fun isKeyword(name: String, project: Project?): Boolean =
        name in JuxKeywords.KEYWORDS

    override fun isIdentifier(name: String, project: Project?): Boolean {
        if (name.isEmpty() || name in JuxKeywords.KEYWORDS) return false
        return isIdentifierShaped(name)
    }

    companion object {
        /**
         * The four Rust words with no raw form (ERRATA E78): the lowered `use`
         * path could not name them, so no path segment may be one.
         */
        val UNNAMEABLE_SEGMENTS = setOf("self", "Self", "crate", "super")

        /**
         * Whether [name] may be a segment of a `package` or `import` path
         * (ERRATA E78, JUX-GRAMMAR-ADDENDUM §A.2.1). Every segment after the
         * first is a member-position name, so a keyword is read as a name there
         * (`package demo.type;`, `import rust.x.record;`). The [first] segment
         * is not, since a path may begin where a statement may begin, so it
         * follows the identifier rule. The words in [UNNAMEABLE_SEGMENTS] are
         * refused everywhere.
         */
        fun isPackageSegment(name: String, first: Boolean): Boolean {
            if (name.isEmpty() || name in UNNAMEABLE_SEGMENTS) return false
            if (first && name in JuxKeywords.KEYWORDS) return false
            return isIdentifierShaped(name)
        }

        private fun isIdentifierShaped(name: String): Boolean {
            if (!(name[0].isLetter() || name[0] == '_')) return false
            return name.all { it.isLetterOrDigit() || it == '_' }
        }
    }
}
