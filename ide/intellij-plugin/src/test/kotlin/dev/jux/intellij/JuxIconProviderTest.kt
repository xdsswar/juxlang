package dev.jux.intellij

import com.intellij.icons.AllIcons
import com.intellij.testFramework.fixtures.BasePlatformTestCase
import javax.swing.Icon

/** Per-content icons: a `.jux` file shows its primary type's glyph, and a folder
 *  holding a `jux.toml` is marked as a module. */
class JuxIconProviderTest : BasePlatformTestCase() {

    private fun iconFor(name: String, code: String): Icon? {
        val file = myFixture.configureByText(name, code)
        return JuxIconProvider().getIcon(file, 0)
    }

    fun testTypeKindsGetTheirGlyph() {
        assertSame(JuxIcons.CLASS, iconFor("Foo.jux", "package demo;\npublic class Foo {}\n"))
        assertSame(JuxIcons.INTERFACE, iconFor("Speaker.jux", "package demo;\npublic interface Speaker {}\n"))
        assertSame(JuxIcons.ENUM, iconFor("Color.jux", "package demo;\npublic enum Color { Red }\n"))
        assertSame(JuxIcons.RECORD, iconFor("Point.jux", "package demo;\npublic record Point(int x) {}\n"))
        assertSame(JuxIcons.STRUCT, iconFor("Vec3.jux", "package demo;\npublic struct Vec3 { public int x; }\n"))
        assertSame(JuxIcons.ANNOTATION, iconFor("Tag.jux", "package demo;\npublic annotation Tag {}\n"))
    }

    /** With several top-level types, the one whose name matches the file wins. */
    fun testPrimaryTypeMatchesFileName() {
        assertSame(
            JuxIcons.INTERFACE,
            iconFor("Speaker.jux", "package demo;\npublic class Helper {}\npublic interface Speaker {}\n"),
        )
    }

    /** Files with no type declaration keep the plain file-type icon (null here). */
    fun testNoTypeFileFallsBack() {
        assertNull(iconFor("main.jux", "package demo;\npublic void main() {}\n"))
        assertNull(
            iconFor("ffi.jux", "package demo;\n@extern(lib = \"c\")\nunsafe native { i32 puts(String s); }\n"),
        )
    }

    /** A directory containing a `jux.toml` is a module; others keep the folder icon. */
    fun testModuleDirectoryGetsModuleIcon() {
        val manifest = myFixture.addFileToProject("modA/jux.toml", "[package]\nname = \"a\"\n")
        val moduleDir = manifest.containingDirectory!!
        assertSame(AllIcons.Nodes.Module, JuxIconProvider().getIcon(moduleDir, 0))

        val plain = myFixture.addFileToProject("plain/notes.txt", "hi")
        assertNull(JuxIconProvider().getIcon(plain.containingDirectory!!, 0))
    }
}
