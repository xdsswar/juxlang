package dev.jux.intellij.highlight

import com.intellij.lang.annotation.AnnotationHolder
import com.intellij.lang.annotation.Annotator
import com.intellij.lang.annotation.HighlightSeverity
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.intentions.JuxImportTypeFix
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.resolve.JuxImports

/**
 * A type name that needs an `import` is an error with the fix attached: red, as
 * in Java, with "Import 'some.Truck'" on Alt+Enter and the auto-import hint
 * over it.
 *
 * When `juxc-lsp` is serving the file it reports the same error itself, so
 * the plugin keeps only the fix there -- two identical messages on one name
 * would be noise, and the fix is what the server cannot offer.
 */
class JuxMissingImportAnnotator : Annotator {
    override fun annotate(element: PsiElement, holder: AnnotationHolder) {
        val type = element.elementType
        if (type !== E.TYPE_REFERENCE && type !== E.REFERENCE_EXPRESSION) return
        // `Outer.Inner`, `pkg.Type`: qualified, so an import is not the question.
        if (element.parent?.elementType === E.FIELD_ACCESS_EXPRESSION && element.parent.firstChild !== element) return
        val name = JuxImports.bareTypeName(element) ?: return
        val candidates = JuxImports.missingImportCandidates(element, name)
        if (candidates.isEmpty()) return
        val fqns = candidates.map { JuxImports.fqnOf(it) }.distinct()
        val serving = dev.jux.intellij.lsp.JuxLspState.isServing(element.project)
        val nameLeaf = element.node.getChildren(null).lastOrNull { it.elementType === JuxTokenTypes.IDENTIFIER }?.psi
            ?: element
        val builder = if (serving) {
            holder.newSilentAnnotation(HighlightSeverity.INFORMATION)
        } else {
            holder.newAnnotation(HighlightSeverity.ERROR, "Cannot resolve symbol '$name'")
        }
        builder.range(nameLeaf)
            .withFix(JuxImportTypeFix(element, name, fqns))
            .create()
    }
}
