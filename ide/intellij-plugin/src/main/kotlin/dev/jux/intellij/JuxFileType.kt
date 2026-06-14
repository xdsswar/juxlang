package dev.jux.intellij

import com.intellij.openapi.fileTypes.LanguageFileType
import javax.swing.Icon

/**
 * Registers `.jux` (user sources) and `*.jux.d` (generated foreign declaration
 * stubs) as a first-class file type with its own icon. The `*.jux.d` pattern is
 * wired in plugin.xml so the stub files land in `FileTypeIndex` and their
 * Rust-crate types/members become resolvable project-wide.
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
