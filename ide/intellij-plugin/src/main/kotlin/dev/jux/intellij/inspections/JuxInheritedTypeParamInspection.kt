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
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * Warns when a class member reuses a SUPERTYPE's type-parameter name (e.g. `T`
 * from `implements Holder<Object>`) where that parameter isn't declared in the
 * current scope. The compiler does not bind it (it leaks a rustc "cannot find
 * type `T`"), so the concrete bound must be written instead:
 *
 * ```jux
 * class HolderName implements Holder<Object> {
 *     public void test(T t) {}     // ← warn: T is not declared here; use Object
 * }
 * ```
 *
 * The quick-fix replaces the bare `T` with its bound (`Object`). A class that
 * actually DECLARES the parameter (`class Box<T> implements Holder<T>`) or a
 * generic method (`<T> …`) is never flagged — that `T` is genuinely in scope.
 */
class JuxInheritedTypeParamInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()

        for (type in PsiTreeUtil.findChildrenOfType(file, JuxTypeDeclaration::class.java)) {
            val bindings = JuxHierarchy.inheritedTypeParameterBindings(type)
            if (bindings.isEmpty()) continue
            val body = type.node.findChildByType(E.CLASS_BODY)?.psi ?: continue

            // Every bare type reference inside this class body whose name is an
            // inherited parameter and is NOT a declared parameter here.
            for (ref in PsiTreeUtil.findChildrenOfType(body, PsiElement::class.java)) {
                if (ref.node.elementType !== E.TYPE_REFERENCE) continue
                // Bare, non-generic, no array/nullable suffixes — a plain `T`.
                val text = ref.text.trim()
                val bound = bindings[text] ?: continue
                // Skip when a nearer scope actually declares it (shadowing).
                if (JuxHierarchy.isDeclaredTypeParameter(ref, text)) continue
                // Skip refs nested in another type's args (only flag the leaf name).
                if (ref.node.findChildByType(E.TYPE_ARGUMENT_LIST) != null) continue

                problems.add(
                    manager.createProblemDescriptor(
                        ref,
                        "Type parameter '$text' is not declared here — use '$bound' " +
                            "(the type bound by the supertype clause)",
                        UseBoundTypeFix(bound),
                        ProblemHighlightType.WARNING,
                        isOnTheFly,
                    ),
                )
            }
        }
        return problems.toTypedArray()
    }

    /** Replaces the bare inherited type-parameter reference with its bound type. */
    private class UseBoundTypeFix(private val bound: String) : LocalQuickFix {
        override fun getFamilyName(): String = "Replace with '$bound'"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val ref = descriptor.psiElement ?: return
            val file = ref.containingFile ?: return
            val docMgr = PsiDocumentManager.getInstance(project)
            val doc = docMgr.getDocument(file) ?: return
            doc.replaceString(ref.textRange.startOffset, ref.textRange.endOffset, bound)
            docMgr.commitDocument(doc)
        }
    }
}
