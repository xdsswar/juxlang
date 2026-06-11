package dev.jux.intellij.resolve

import com.intellij.codeInsight.daemon.LineMarkerInfo
import com.intellij.codeInsight.daemon.LineMarkerProviderDescriptor
import com.intellij.codeInsight.navigation.NavigationGutterIconBuilder
import com.intellij.icons.AllIcons
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxMethodDeclaration
import javax.swing.Icon

/**
 * "Overrides / implements" up-arrow gutter markers on Jux methods, the
 * Java-plugin staple. A method gets a marker when an enclosing class's
 * supertype chain (resolved project-wide via [JuxHierarchy] + [JuxTypeIndex])
 * declares a method with the same name and arity:
 *
 * - super has a **body** → overriding (↑ solid arrow), navigates to it;
 * - super is **abstract** (interface method / no body) → implementing
 *   (↑ hollow arrow).
 *
 * Down-arrows ("overridden by") need a reverse index over the whole project
 * per method and are deferred until a stub index lands (plugin-gap.md B4).
 */
class JuxLineMarkerProvider : LineMarkerProviderDescriptor() {
    override fun getName(): String = "Overriding / implementing method"

    override fun getIcon(): Icon = AllIcons.Gutter.OverridingMethod

    override fun getLineMarkerInfo(element: PsiElement): LineMarkerInfo<*>? {
        // Markers must sit on leaf elements (platform contract): the method's
        // name identifier.
        if (element.elementType !== JuxTokenTypes.IDENTIFIER) return null
        val method = element.parent as? JuxMethodDeclaration ?: return null
        if (method.nameIdentifier !== element) return null
        val name = method.name ?: return null

        val owner = JuxHierarchy.enclosingType(method) ?: return null
        val superMethod =
            JuxHierarchy.findSuperMethod(owner, name, JuxHierarchy.arity(method)) ?: return null

        val overrides = JuxHierarchy.hasBody(superMethod)
        val icon = if (overrides) AllIcons.Gutter.OverridingMethod else AllIcons.Gutter.ImplementingMethod
        val verb = if (overrides) "Overrides" else "Implements"
        val superOwner = JuxHierarchy.enclosingType(superMethod)?.name ?: "supertype"

        return NavigationGutterIconBuilder.create(icon)
            .setTargets(superMethod)
            .setTooltipText("$verb method in '$superOwner'")
            .createLineMarkerInfo(element)
    }
}
