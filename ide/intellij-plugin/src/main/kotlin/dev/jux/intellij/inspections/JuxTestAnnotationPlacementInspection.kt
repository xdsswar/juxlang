package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.run.JuxTestDetector

/**
 * §TS.1 placement rules for the testing-framework annotations (`@Test`,
 * `@BeforeAll`, `@BeforeEach`, `@AfterEach`, `@AfterAll`), mirrored IDE-side so
 * the error shows before `jux test` does:
 *
 *  - only on **free functions** — never on class/interface methods;
 *  - the function must take **no parameters**;
 *  - the return type must be `void` (`async void` is fine — §TS.6).
 *
 * Annotation casing never matters (`@test` ≡ `@Test`). The quick-fix removes
 * the offending annotation — the compiler's own escape hatch.
 */
class JuxTestAnnotationPlacementInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()

        for (method in PsiTreeUtil.findChildrenOfType(file, JuxMethodDeclaration::class.java)) {
            val anns = JuxTestDetector.testAnnotations(method)
            if (anns.isEmpty()) continue
            val annName = "@${anns.first()}"
            // Highlight the annotation node itself (first matching one).
            val target = annotationNode(method) ?: method.nameIdentifier ?: method

            fun report(message: String) {
                problems.add(
                    manager.createProblemDescriptor(
                        target, message, RemoveAnnotationFix(annName),
                        ProblemHighlightType.ERROR, isOnTheFly,
                    ),
                )
            }

            if (!JuxTestDetector.isFreeFunction(method)) {
                report("$annName is only valid on free functions, not methods (sec. TS.1)")
                continue
            }
            if (hasParameters(method)) {
                report("$annName function must take no parameters (sec. TS.1)")
                continue
            }
            if (!returnsVoid(method)) {
                report("$annName function must return void (sec. TS.1)")
            }
        }
        return problems.toTypedArray()
    }

    /** The method's first §TS annotation node (for the highlight range). */
    private fun annotationNode(method: JuxMethodDeclaration): PsiElement? {
        var c: PsiElement? = method.firstChild
        while (c != null) {
            if (c.elementType === E.ANNOTATION &&
                c.text.removePrefix("@").substringBefore('(').trim().lowercase()
                    in JuxTestDetector.TEST_HOOKS
            ) return c
            c = c.nextSibling
        }
        return null
    }

    private fun hasParameters(method: JuxMethodDeclaration): Boolean {
        val list = method.node.findChildByType(E.PARAMETER_LIST)?.psi ?: return false
        return list.children.any { it.elementType === E.PARAMETER }
    }

    /**
     * The declared return type is the method's first TYPE_REFERENCE direct
     * child (it precedes the name; parameter types are nested inside the
     * PARAMETER_LIST, never direct children). `void` parses as a TYPE_REFERENCE
     * holding just the keyword.
     */
    private fun returnsVoid(method: JuxMethodDeclaration): Boolean {
        val ret = method.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return true
        return ret.text.trim() == "void"
    }

    /** Deletes the annotation line (annotation text plus its trailing newline indent). */
    private class RemoveAnnotationFix(private val annName: String) : LocalQuickFix {
        override fun getFamilyName(): String = "Remove $annName annotation"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val ann = descriptor.psiElement?.takeIf { it.elementType === E.ANNOTATION } ?: return
            val file = ann.containingFile ?: return
            val docMgr = PsiDocumentManager.getInstance(project)
            val doc = docMgr.getDocument(file) ?: return
            var end = ann.textRange.endOffset
            // Swallow the line break after the annotation so no blank line remains.
            val text = doc.charsSequence
            while (end < text.length && (text[end] == ' ' || text[end] == '\t')) end++
            if (end < text.length && text[end] == '\n') end++
            doc.deleteString(ann.textRange.startOffset, end)
            docMgr.commitDocument(doc)
        }
    }
}
