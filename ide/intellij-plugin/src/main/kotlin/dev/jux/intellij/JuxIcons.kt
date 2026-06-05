package dev.jux.intellij

import com.intellij.openapi.util.IconLoader

/**
 * Icon handles for the Jux plugin.
 *
 * [FILE] is the file-type icon (project view + editor tabs). The rest are the
 * per-kind icons shown next to each entry in the New → Jux File dialog (§I.5),
 * mirroring Java's class/interface/enum/… glyphs.
 *
 * Icons ship as 16×16 PNGs with a 32×32 `@2x` variant for HiDPI; `IconLoader`
 * picks the right one automatically.
 */
object JuxIcons {
    /** `.jux` file-type icon. */
    @JvmField
    val FILE = load("/icons/jux.png")

    /** New → "File" (plain source file) kind. */
    @JvmField
    val NEW_FILE = load("/icons/juxFile.png")

    @JvmField
    val CLASS = load("/icons/juxClass.png")

    @JvmField
    val INTERFACE = load("/icons/juxInterface.png")

    @JvmField
    val ENUM = load("/icons/juxEnum.png")

    @JvmField
    val STRUCT = load("/icons/juxStruct.png")

    @JvmField
    val RECORD = load("/icons/juxRecord.png")

    @JvmField
    val ANNOTATION = load("/icons/juxAnnotation.png")

    private fun load(path: String) = IconLoader.getIcon(path, JuxIcons::class.java)
}
