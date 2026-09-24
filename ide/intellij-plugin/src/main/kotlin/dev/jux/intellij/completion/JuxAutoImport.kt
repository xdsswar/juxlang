package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.InsertHandler
import com.intellij.codeInsight.completion.InsertionContext
import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.openapi.editor.Document
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import dev.jux.intellij.JuxPackageResolver
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration

/**
 * Auto-import on completion accept (the IDE-side counterpart of `juxc-lsp`'s
 * `additionalTextEdits`): when the user picks a project type that lives in
 * another package and isn't imported yet, insert `import a.b.Type;` in the
 * right place. Without the LSP this is what makes cross-file types usable from
 * completion instead of red-underlined.
 */
object JuxAutoImport {

    /** The dot-joined package a type is declared in, or `""` at the file root. */
    fun packageOf(type: JuxTypeDeclaration): String =
        type.containingFile?.let { packageOfFile(it) } ?: ""

    /** The dot-joined `package …;` of a file, or `""` when it has none. */
    fun packageOfFile(file: PsiFile): String {
        val pkg = file.children.firstOrNull { it.elementType === E.PACKAGE_STATEMENT } ?: return ""
        return PACKAGE_RE.find(pkg.text)?.groupValues?.get(1)?.trim().orEmpty()
    }

    /**
     * The package a file's names actually resolve in: its `package …;` when it
     * declares one, else the package its LOCATION implies.
     *
     * A file with no `package` line is not necessarily in the default package:
     * its package comes from where it sits under its source root
     * ([JuxPackageResolver.inferPackage]), which is how the compiler and the
     * language server read it. Used wherever "is this name already visible
     * without an import?" is decided, so a sibling in the same directory is
     * never offered an import even when one of the two files omits the line.
     *
     * Falls back to `""` (the default package) whenever the layout cannot say,
     * which is the safe direction here: it can only make the answer "already
     * visible" more often, never less.
     */
    fun effectivePackageOfFile(file: PsiFile): String {
        val declared = packageOfFile(file)
        if (declared.isNotEmpty()) return declared
        val vf = file.originalFile.virtualFile ?: return ""
        return try {
            JuxPackageResolver.inferPackage(vf, file.project).orEmpty()
        } catch (_: Throwable) {
            // Index not ready, or a file outside every root: "no idea" is "".
            ""
        }
    }

    /** [effectivePackageOfFile] for the file a type is declared in. */
    fun effectivePackageOf(type: JuxTypeDeclaration): String =
        type.containingFile?.let { effectivePackageOfFile(it) } ?: ""

    /**
     * True when [type] needs no `import` in [file]: it is declared in that very
     * file, or in the file's own package.
     *
     * The rule the whole import surface shares, so completion, the "Import
     * type" fix, the on-the-fly importer and the unused-import analysis cannot
     * disagree about it. Mirrors `juxc-lsp`'s `auto_import_for`, whose
     * `same_package_type_is_offered_no_import` test pins the same two cases:
     * a sibling in the same package, and a class importing itself.
     */
    fun needsNoImport(file: PsiFile, type: JuxTypeDeclaration): Boolean {
        val declaringFile = type.containingFile ?: return true
        if (declaringFile.originalFile == file.originalFile) return true
        return effectivePackageOf(type) == effectivePackageOfFile(file)
    }

    /**
     * An [InsertHandler] that adds `import <fqn>;` after accept, unless the file
     * already imports that name or sits in the same package. Null [fqn] (same
     * package / no package) means "no import needed" — callers pass the handler
     * only when an import is actually wanted.
     */
    fun handler(fqn: String, simpleName: String): InsertHandler<LookupElement> =
        InsertHandler { context, _ -> insertImport(context, fqn, simpleName) }

    private fun insertImport(context: InsertionContext, fqn: String, simpleName: String) {
        val file = context.file as? JuxFile ?: return
        addImport(context.project, context.document, file, fqn, simpleName)
    }

    /**
     * Insert `import <fqn>;` into [file] at the canonical spot (after the last
     * import, else after `package …;`, else file start), unless [simpleName] is
     * already imported. Shared by the completion insert handler and the
     * "Import type" quick-fix. The caller must hold a write action.
     */
    fun addImport(project: Project, document: Document, file: JuxFile, fqn: String, simpleName: String) {
        if (importAlreadyPresent(file, fqn, simpleName)) return
        if (collapseIntoWildcard(project, document, file, fqn)) return
        val offset = importInsertOffset(file)
        val nl = if (offset == 0) "" else "\n"
        // A blank line after the package block reads better when we're the
        // first import; otherwise just append on its own line.
        document.insertString(offset, "${nl}import $fqn;")
        PsiDocumentManager.getInstance(project).commitDocument(document)
    }

    /**
     * The Imports tab's wildcard threshold (Code Style | Jux): when this import
     * would make it that many single-name imports from one package, they all
     * become `import pkg.*;` instead. True when that happened (the name is now
     * imported); false when the threshold is off or not reached.
     */
    private fun collapseIntoWildcard(project: Project, document: Document, file: JuxFile, fqn: String): Boolean {
        val threshold = dev.jux.intellij.format.JuxCodeStyleSettings.of(file).NAMES_COUNT_TO_USE_WILDCARD
        val pkg = fqn.substringBeforeLast('.', "")
        if (threshold <= 0 || pkg.isEmpty()) return false
        val single = Regex("""^import\s+${Regex.escape(pkg)}\.\w+\s*;$""")
        val same = file.children.filter { it.elementType === E.IMPORT_STATEMENT && single.matches(it.text.trim()) }
        if (same.size + 1 < threshold) return false
        // Back to front: the first import's line becomes the wildcard, the
        // others go with the line break in front of them.
        for (imp in same.drop(1).asReversed()) {
            val start = imp.textRange.startOffset
            var from = start
            while (from > 0 && document.charsSequence[from - 1].let { it == ' ' || it == '\t' }) from--
            if (from > 0 && document.charsSequence[from - 1] == '\n') from--
            document.deleteString(from, imp.textRange.endOffset)
        }
        val first = same.firstOrNull()
        if (first != null) {
            document.replaceString(first.textRange.startOffset, first.textRange.endOffset, "import $pkg.*;")
        } else {
            val offset = importInsertOffset(file)
            document.insertString(offset, "${if (offset == 0) "" else "\n"}import $pkg.*;")
        }
        PsiDocumentManager.getInstance(project).commitDocument(document)
        return true
    }

    /** True when [simpleName] (from [fqn]) is already imported by [file]. */
    fun isImported(file: JuxFile, fqn: String, simpleName: String): Boolean =
        importAlreadyPresent(file, fqn, simpleName)

    /** True when [simpleName] is already brought in by some import in [file]. */
    private fun importAlreadyPresent(file: JuxFile, fqn: String, simpleName: String): Boolean {
        for (imp in file.children) {
            if (imp.elementType !== E.IMPORT_STATEMENT) continue
            val t = imp.text
            // Exact FQN, a wildcard over its package, or a grouped/aliased
            // bind of the simple name — any of these already supplies it.
            if (t.contains("$fqn;") || t.contains("$fqn ")) return true
            val pkg = fqn.substringBeforeLast('.', "")
            if (pkg.isNotEmpty() && t.contains("$pkg.*")) return true
            if (Regex("""\b${Regex.escape(simpleName)}\b""").containsMatchIn(t) &&
                t.contains("import ")
            ) {
                // Conservative: a grouped import listing the name. Only treat as
                // present when the package prefix matches too, to avoid a
                // same-name-different-package false positive.
                if (pkg.isEmpty() || t.contains(pkg)) return true
            }
        }
        return false
    }

    /**
     * Where a new `import` line goes: just after the last existing import, else
     * after the `package …;` statement, else file start. Returns the offset at
     * the END of that anchor line (the inserted text leads with its own `\n`).
     */
    private fun importInsertOffset(file: JuxFile): Int {
        var anchor: PsiElement? = null
        for (child in file.children) {
            when (child.elementType) {
                E.PACKAGE_STATEMENT, E.IMPORT_STATEMENT -> anchor = child
                else -> if (anchor != null) return anchor.textRange.endOffset
            }
        }
        return anchor?.textRange?.endOffset ?: 0
    }

    private val PACKAGE_RE = Regex("""package\s+([A-Za-z_][\w.]*)\s*;""")
}
