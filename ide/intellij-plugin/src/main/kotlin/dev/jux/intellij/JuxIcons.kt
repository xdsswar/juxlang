package dev.jux.intellij

import com.intellij.openapi.util.IconLoader

/**
 * Icon handles for the Jux plugin.
 *
 * [FILE] is the file-type icon (project view + editor tabs). The rest are the
 * per-kind icons shown next to each entry in the New → Jux File dialog (§I.5),
 * mirroring Java's class/interface/enum/… glyphs.
 *
 * Icons are clean, hand-authored line-art SVGs (16×16 viewBox) — single-color
 * and theme-adaptive: a charcoal stroke for light themes plus a `_dark` sibling
 * (soft near-white) that `IconLoader` auto-selects on dark/Dracula themes.
 * Scalable, so no `@2x` raster variants are needed.
 */
object JuxIcons {
    /** `.jux` file-type icon. */
    @JvmField
    val FILE = load("/icons/jux.svg")

    /**
     * Tool-window stripe icon (13×13, monochrome): dark on light themes, white
     * on dark themes (via the `_dark` sibling), so it dims when the tool window
     * is unselected and reads as the theme foreground when selected.
     */
    @JvmField
    val TOOL_WINDOW = load("/icons/juxToolWindow.svg")

    /** New → "File" (plain source file) kind. */
    @JvmField
    val NEW_FILE = load("/icons/juxFile.svg")

    @JvmField
    val CLASS = load("/icons/juxClass.svg")

    @JvmField
    val INTERFACE = load("/icons/juxInterface.svg")

    @JvmField
    val ENUM = load("/icons/juxEnum.svg")

    @JvmField
    val STRUCT = load("/icons/juxStruct.svg")

    @JvmField
    val RECORD = load("/icons/juxRecord.svg")

    @JvmField
    val ANNOTATION = load("/icons/juxAnnotation.svg")

    // §P.7.8 property gutter trio (12×12 SVGs with `_dark` variants —
    // IconLoader picks the themed file automatically).

    /** Gutter: the property has observers attached somewhere. */
    @JvmField
    val PROPERTY_OBSERVED = load("/icons/propertyObserved.svg")

    /** Gutter: the property is bound, or is a binding source. */
    @JvmField
    val PROPERTY_BOUND = load("/icons/propertyBound.svg")

    /** Gutter: the property is neither observed nor bound. */
    @JvmField
    val PROPERTY_PLAIN = load("/icons/propertyPlain.svg")

    private fun load(path: String) = IconLoader.getIcon(path, JuxIcons::class.java)
}
