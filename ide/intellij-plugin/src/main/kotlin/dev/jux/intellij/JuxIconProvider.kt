package dev.jux.intellij

import com.intellij.ide.IconProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import javax.swing.Icon

/**
 * Per-content file icons (Project View, editor tabs, navigation bar).
 *
 * The flat file-type icon ([JuxIcons.FILE]) is the same for every `.jux`, which
 * makes a project of classes/interfaces/enums look uniform. Mirroring how Java
 * shows a class/interface/enum glyph per file, this picks the icon of the file's
 * **primary** top-level type declaration: the one whose name matches the file's
 * base name, else the first type declared. Files with no type declaration (free
 * functions, a bare `unsafe native { … }` FFI block) return `null` so the
 * platform falls back to the plain file-type icon.
 */
class JuxIconProvider : IconProvider() {
    override fun getIcon(element: PsiElement, flags: Int): Icon? {
        val file = element as? JuxFile ?: return null
        val types = file.children.filter { it.elementType in TYPE_ICONS }
        if (types.isEmpty()) return null
        val base = file.name.removeSuffix(".jux")
        val primary = types.firstOrNull { (it as? JuxNamedElement)?.name == base } ?: types.first()
        return TYPE_ICONS[primary.elementType]
    }

    private companion object {
        val TYPE_ICONS = mapOf(
            E.CLASS_DECLARATION to JuxIcons.CLASS,
            E.STRUCT_DECLARATION to JuxIcons.STRUCT,
            E.INTERFACE_DECLARATION to JuxIcons.INTERFACE,
            E.ENUM_DECLARATION to JuxIcons.ENUM,
            E.RECORD_DECLARATION to JuxIcons.RECORD,
            E.ANNOTATION_DECLARATION to JuxIcons.ANNOTATION,
        )
    }
}
