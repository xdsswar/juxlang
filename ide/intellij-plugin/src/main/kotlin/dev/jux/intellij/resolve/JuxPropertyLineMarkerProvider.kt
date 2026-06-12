package dev.jux.intellij.resolve

import com.intellij.codeInsight.daemon.LineMarkerInfo
import com.intellij.codeInsight.daemon.LineMarkerProviderDescriptor
import com.intellij.codeInsight.navigation.NavigationGutterIconBuilder
import com.intellij.openapi.editor.markup.GutterIconRenderer
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiManager
import com.intellij.psi.search.FileTypeIndex
import com.intellij.psi.search.GlobalSearchScope
import com.intellij.psi.util.elementType
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.JuxIcons
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxPropertyDeclaration
import javax.swing.Icon

/**
 * §P.7.8 — the property gutter trio on every `{ get; set; }` declaration:
 *
 * - **observed** (filled dot) — `.observers.attach(…)` sites exist for it;
 * - **bound** (link) — it is a `bind`/`bindBidirectional` receiver or appears
 *   as a binding *source* (the argument side);
 * - **plain** (hollow square) — neither. Specified, but noisy on big files —
 *   each kind has its own gutter [Option] so users can switch it off in
 *   Settings → Editor → General → Gutter Icons.
 *
 * Clicking navigates to the attach/bind call sites (the existing
 * Find-Usages-style [NavigationGutterIconBuilder] infra).
 *
 * Everything runs in [collectSlowLineMarkers] (the daemon's second pass, like
 * Java's "overridden by" markers): status is the union of every project file's
 * cached [JuxPropertyUsages] scan, aggregated once per pass — the cost profile
 * [JuxTypeIndex] already accepts for the override markers. Matching is by the
 * property's final name (same heuristic as the scan itself); a same-named
 * property on another class can therefore borrow the status — documented,
 * resolved fully only once member resolution lands.
 */
class JuxPropertyLineMarkerProvider : LineMarkerProviderDescriptor() {
    override fun getName(): String = "Observable property status"

    override fun getIcon(): Icon = JuxIcons.PROPERTY_OBSERVED

    private val observedOption = Option("jux.property.observed", "Observed property", JuxIcons.PROPERTY_OBSERVED)
    private val boundOption = Option("jux.property.bound", "Bound property", JuxIcons.PROPERTY_BOUND)
    private val plainOption = Option("jux.property.plain", "Unobserved, unbound property", JuxIcons.PROPERTY_PLAIN)

    override fun getOptions(): Array<Option> = arrayOf(observedOption, boundOption, plainOption)

    /** Fast pass: nothing — all statuses need the project-wide aggregate. */
    override fun getLineMarkerInfo(element: PsiElement): LineMarkerInfo<*>? = null

    override fun collectSlowLineMarkers(
        elements: List<PsiElement>,
        result: MutableCollection<in LineMarkerInfo<*>>,
    ) {
        // Markers sit on leaf elements (platform contract): the name identifier.
        val props = elements.mapNotNull { e ->
            if (e.elementType !== JuxTokenTypes.IDENTIFIER) return@mapNotNull null
            val prop = e.parent as? JuxPropertyDeclaration ?: return@mapNotNull null
            if (prop.nameIdentifier !== e) return@mapNotNull null
            e to prop
        }
        if (props.isEmpty()) return

        // Aggregate every project file's cached §P scan ONCE for this batch.
        val project = props.first().second.project
        val attach = HashMap<String, MutableList<PsiElement>>()
        val bind = HashMap<String, MutableList<PsiElement>>()
        val sources = HashSet<String>()
        val manager = PsiManager.getInstance(project)
        for (vf in FileTypeIndex.getFiles(JuxFileType, GlobalSearchScope.projectScope(project))) {
            val psi = manager.findFile(vf) ?: continue
            val usages = JuxPropertyUsages.usagesIn(psi)
            usages.attachSites.forEach { (n, sites) -> attach.getOrPut(n) { ArrayList() }.addAll(sites) }
            usages.bindSites.forEach { (n, sites) -> bind.getOrPut(n) { ArrayList() }.addAll(sites) }
            sources.addAll(usages.bindSources)
        }

        for ((leaf, prop) in props) {
            val name = prop.name ?: continue
            val attachSites = attach[name].orEmpty()
            val bindSites = bind[name].orEmpty()
            val isBound = bindSites.isNotEmpty() || name in sources

            val marker = when {
                // Binding is the stronger structural fact — it wins when both.
                isBound && boundOption.isEnabled -> navigable(
                    leaf,
                    JuxIcons.PROPERTY_BOUND,
                    "Property '$name' is bound — click for binding sites",
                    bindSites.ifEmpty { attachSites },
                )
                !isBound && attachSites.isNotEmpty() && observedOption.isEnabled -> navigable(
                    leaf,
                    JuxIcons.PROPERTY_OBSERVED,
                    "Property '$name' is observed — click for attach sites",
                    attachSites,
                )
                !isBound && attachSites.isEmpty() && plainOption.isEnabled ->
                    LineMarkerInfo(
                        leaf,
                        leaf.textRange,
                        JuxIcons.PROPERTY_PLAIN,
                        { "Property '$name' is not observed or bound" },
                        null,
                        GutterIconRenderer.Alignment.LEFT,
                        { "Unobserved property" },
                    )
                else -> null
            }
            marker?.let(result::add)
        }
    }

    private fun navigable(
        leaf: PsiElement,
        icon: Icon,
        tooltip: String,
        targets: List<PsiElement>,
    ): LineMarkerInfo<*> =
        NavigationGutterIconBuilder.create(icon)
            .setTargets(targets)
            .setTooltipText(tooltip)
            .createLineMarkerInfo(leaf)
}
