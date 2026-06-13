package dev.jux.intellij.resolve

import com.intellij.codeInsight.daemon.LineMarkerInfo
import com.intellij.codeInsight.daemon.LineMarkerProviderDescriptor
import com.intellij.codeInsight.navigation.NavigationGutterIconBuilder
import com.intellij.icons.AllIcons
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxTypeDeclaration
import javax.swing.Icon

/**
 * The DOWN-arrow counterpart of [JuxLineMarkerProvider]: "is subclassed /
 * implemented / overridden by" gutters that navigate from a supertype (or a
 * super method) to its subtypes / overriding methods — the Java staple where
 * clicking the icon pops a list of the implementors.
 *
 * - on a **class** name → its subclasses (↓ "is subclassed by");
 * - on an **interface** name → its implementors (↓ "is implemented by");
 * - on a **method** name → the methods that override/implement it in subtypes
 *   (↓ "is overridden in" / "is implemented in").
 *
 * Built as SLOW markers ([collectSlowLineMarkers]) because answering "who
 * extends me?" is a project-wide query — it runs on the daemon's background
 * pass, off the typing path. The reverse index is computed once per pass from
 * [JuxTypeIndex] (whose per-file type lists are cached) and walked transitively,
 * so deep hierarchies (A ← B ← C) all light up. Targets resolve by simple name,
 * matching the rest of the plugin's IDE-side resolution.
 */
class JuxSubtypeLineMarkerProvider : LineMarkerProviderDescriptor() {
    override fun getName(): String = "Subclassed / overridden (subtypes)"
    override fun getIcon(): Icon = AllIcons.Gutter.OverridenMethod

    // Everything here is a project-wide query → only the slow pass.
    override fun getLineMarkerInfo(element: PsiElement): LineMarkerInfo<*>? = null

    override fun collectSlowLineMarkers(
        elements: List<PsiElement>,
        result: MutableCollection<in LineMarkerInfo<*>>,
    ) {
        val first = elements.firstOrNull() ?: return
        // Reverse index built once per pass; per-element lookups are cheap.
        val index = JuxSubtypes.buildIndex(first.project)
        if (index.isEmpty()) return

        for (element in elements) {
            if (element.elementType !== JuxTokenTypes.IDENTIFIER) continue
            when (val parent = element.parent) {
                is JuxTypeDeclaration -> {
                    if (parent.nameIdentifier !== element) continue
                    val name = parent.name ?: continue
                    val subs = JuxSubtypes.transitiveSubtypes(name, index)
                    if (subs.isEmpty()) continue
                    val iface = JuxHierarchy.isInterface(parent)
                    addMarker(
                        result, element,
                        if (iface) AllIcons.Gutter.ImplementedMethod else AllIcons.Gutter.OverridenMethod,
                        if (iface) "Is implemented by" else "Is subclassed by",
                        subs.mapNotNull { it.nameIdentifier },
                    )
                }
                is JuxMethodDeclaration -> {
                    if (parent.nameIdentifier !== element) continue
                    val owner = JuxHierarchy.enclosingType(parent) ?: continue
                    val overrides = JuxSubtypes
                        .overridingMethods(owner, parent.name ?: continue, JuxHierarchy.arity(parent), index)
                        .mapNotNull { it.nameIdentifier }
                    if (overrides.isEmpty()) continue
                    val abstractHere = JuxHierarchy.isAbstractMethod(parent)
                    addMarker(
                        result, element,
                        if (abstractHere) AllIcons.Gutter.ImplementedMethod else AllIcons.Gutter.OverridenMethod,
                        if (abstractHere) "Is implemented in" else "Is overridden in",
                        overrides,
                    )
                }
                else -> {}
            }
        }
    }

    private fun addMarker(
        result: MutableCollection<in LineMarkerInfo<*>>,
        anchor: PsiElement,
        icon: Icon,
        verb: String,
        targets: List<PsiElement>,
    ) {
        if (targets.isEmpty()) return
        val noun = if (targets.size == 1) "1 place" else "${targets.size} places"
        result.add(
            NavigationGutterIconBuilder.create(icon)
                .setTargets(targets)
                .setTooltipText("$verb ($noun)")
                .createLineMarkerInfo(anchor),
        )
    }
}
