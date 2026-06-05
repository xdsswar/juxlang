package dev.jux.intellij

import com.intellij.openapi.fileTypes.LanguageFileType
import javax.swing.Icon

/**
 * Registers `.jux` as a first-class file type with its own icon.
 *
 * Backed by [JuxLanguage], which has no parser in Phase 1 — highlighting is
 * delegated to the bundled TextMate grammar and semantics to `juxc-lsp`.
 */
object JuxFileType : LanguageFileType(JuxLanguage) {
    override fun getName(): String = "Jux File"
    override fun getDescription(): String = "Jux source file"
    override fun getDefaultExtension(): String = "jux"
    override fun getIcon(): Icon = JuxIcons.FILE
    // The `fieldName="INSTANCE"` in plugin.xml resolves to the static
    // `INSTANCE` field Kotlin generates for every `object`.
}
