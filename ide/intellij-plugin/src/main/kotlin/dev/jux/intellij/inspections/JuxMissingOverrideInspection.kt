package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.editor.Document
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * A method that redeclares a supertype method (same name + arity, found by
 * the project-wide [JuxHierarchy] walk) but carries no `@override` annotation.
 * Jux annotations are case-insensitive, so `@Override` / `@OVERRIDE` count.
 * Quick-fix inserts `@override` on its own line above the method.
 */
class JuxMissingOverrideInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()

        for (method in PsiTreeUtil.findChildrenOfType(file, JuxMethodDeclaration::class.java)) {
            val name = method.name ?: continue
            if (hasOverrideAnnotation(method)) continue
            if (!JuxHierarchy.isOverridable(method)) continue // static/private/final never override
            val owner = JuxHierarchy.enclosingType(method) ?: continue
            val superMethod =
                JuxHierarchy.findSuperMethod(owner, name, JuxHierarchy.arity(method)) ?: continue
            val superName = JuxHierarchy.enclosingType(superMethod)?.name ?: "supertype"
            val target = method.nameIdentifier ?: continue

            problems.add(
                manager.createProblemDescriptor(
                    target,
                    "Method '$name' overrides a method of '$superName' but is not annotated @override",
                    AddOverrideFix(),
                    ProblemHighlightType.WEAK_WARNING,
                    isOnTheFly,
                ),
            )
        }
        return problems.toTypedArray()
    }

    private fun hasOverrideAnnotation(method: JuxMethodDeclaration): Boolean {
        var c: PsiElement? = method.firstChild
        while (c != null) {
            if (c.elementType === E.ANNOTATION &&
                c.text.removePrefix("@").substringBefore('(').trim()
                    .equals("override", ignoreCase = true)
            ) return true
            c = c.nextSibling
        }
        return false
    }

    /** Inserts `@override` on its own line above the method, matching its indent. */
    private class AddOverrideFix : LocalQuickFix {
        override fun getFamilyName(): String = "Add @override"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val method = descriptor.psiElement?.parent ?: return // name id → method decl
            val file = method.containingFile ?: return
            val docMgr = PsiDocumentManager.getInstance(project)
            val doc = docMgr.getDocument(file) ?: return
            val start = method.textRange.startOffset
            doc.insertString(start, "@override\n" + indentAt(doc, start))
            docMgr.commitDocument(doc)
        }

        /** The whitespace prefix of the line containing [offset]. */
        private fun indentAt(doc: Document, offset: Int): String {
            val lineStart = doc.getLineStartOffset(doc.getLineNumber(offset))
            val prefix = doc.charsSequence.subSequence(lineStart, offset)
            return prefix.takeWhile { it == ' ' || it == '\t' }.toString()
        }
    }
}
