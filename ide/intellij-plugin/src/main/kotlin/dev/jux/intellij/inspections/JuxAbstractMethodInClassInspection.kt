package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.quickfix.JuxMakeClassAbstractFix
import dev.jux.intellij.quickfix.JuxMakeMethodNotAbstractFix
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * "Abstract method in non-abstract class", Java's compile error: a class
 * that declares an `abstract` method must itself be `abstract`, since an
 * instance would have a method with no body.
 *
 * Only an explicit `abstract` on a method of a `class` is checked; interface
 * methods are abstract by nature, and enums and records are not classes
 * here. The fixes are Java's: make the class abstract, or give the method a
 * body.
 */
class JuxAbstractMethodInClassInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                if (element.elementType !== E.METHOD_DECLARATION) return
                if (!JuxHierarchy.hasModifier(element, "abstract")) return
                val body = element.parent ?: return
                if (body.elementType !== E.CLASS_BODY) return
                val type = body.parent as? JuxTypeDeclaration ?: return
                if (!JuxHierarchy.isClass(type) || JuxHierarchy.hasModifier(type, "abstract")) return
                val name = (element as? JuxNamedElement)?.name ?: return
                val anchor = (element as? JuxNamedElement)?.nameIdentifier ?: return
                holder.registerProblem(
                    anchor,
                    "Abstract method '$name' in non-abstract class '${type.name}'",
                    ProblemHighlightType.GENERIC_ERROR,
                    JuxMakeClassAbstractFix(type),
                    JuxMakeMethodNotAbstractFix(element, name),
                )
            }
        }
}
