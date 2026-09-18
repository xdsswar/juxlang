package dev.jux.intellij.completion

import com.intellij.codeInsight.daemon.ReferenceImporter
import com.intellij.openapi.editor.Editor
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.resolve.JuxImports
import dev.jux.intellij.settings.JuxCodeInsightSettings
import java.util.function.BooleanSupplier

/**
 * "Add unambiguous imports on the fly" (Settings | Editor | General | Auto
 * Import | Jux): a type name with exactly one importable declaration gets its
 * `import` written as soon as it is typed. The option is Jux's own, as
 * Kotlin's is, so it works in IDEs without the Java plugin.
 */
class JuxReferenceImporter : ReferenceImporter {

    override fun isAddUnambiguousImportsOnTheFlyEnabled(file: PsiFile): Boolean =
        file is JuxFile && JuxCodeInsightSettings.getInstance().addUnambiguousImportsOnTheFly

    override fun computeAutoImportAtOffset(
        editor: Editor,
        file: PsiFile,
        offset: Int,
        allowCaretNearReference: Boolean,
    ): BooleanSupplier? {
        if (file !is JuxFile) return null
        val use = typeUseAt(file, offset) ?: return null
        val range = use.textRange
        if (!allowCaretNearReference && editor.caretModel.offset in range.startOffset..range.endOffset) return null
        val name = JuxImports.bareTypeName(use) ?: return null
        val candidate = JuxImports.missingImportCandidates(use, name).singleOrNull() ?: return null
        val fqn = JuxImports.fqnOf(candidate)
        return BooleanSupplier {
            val document = file.viewProvider.document ?: return@BooleanSupplier false
            JuxAutoImport.addImport(file.project, document, file, fqn, name)
            true
        }
    }

    private fun typeUseAt(file: JuxFile, offset: Int): PsiElement? {
        var e: PsiElement? = file.findElementAt(offset)
        while (e != null && e !is JuxFile) {
            if (e.elementType === E.TYPE_REFERENCE || e.elementType === E.REFERENCE_EXPRESSION) return e
            e = e.parent
        }
        return null
    }
}
