package dev.jux.intellij.project

import com.intellij.icons.AllIcons
import com.intellij.ide.projectView.actions.MarkSourceRootAction
import com.intellij.openapi.actionSystem.CustomShortcutSet
import com.intellij.openapi.roots.ui.configuration.ModuleSourceRootEditHandler
import com.intellij.ui.JBColor
import org.jetbrains.jps.model.JpsDummyElement
import org.jetbrains.jps.model.ex.JpsElementTypeWithDummyProperties
import org.jetbrains.jps.model.module.JpsModuleSourceRootType
import org.jetbrains.jps.model.serialization.JpsModelSerializerExtension
import org.jetbrains.jps.model.serialization.module.JpsModuleSourceRootDummyPropertiesSerializer
import org.jetbrains.jps.model.serialization.module.JpsModuleSourceRootPropertiesSerializer
import java.awt.Color
import javax.swing.Icon

/**
 * The two Jux source-root kinds (§I.4): **Jux Sources Root** for production
 * code and **Jux Test Sources Root** for tests.
 *
 * A directory marked with either is a package root: a file's package is its
 * directory's path below the root, dotted (`src/com/example/Foo.jux` is in
 * `com.example`). They are distinct from Java's source roots so a Jux project
 * never borrows Java semantics (a Java root carries a package prefix and a
 * compile classpath Jux has no use for), and so an IDE without the Java
 * plugin can still mark them.
 *
 * The types carry no properties ([JpsDummyElement]); what makes them
 * persistent is [JuxJpsModelSerializerExtension], which gives each a stable id
 * in the module file.
 */
class JuxSourceRootType private constructor(private val forTests: Boolean) :
    JpsElementTypeWithDummyProperties(), JpsModuleSourceRootType<JpsDummyElement> {

    /** A test root counts as test content: the IDE runs, colors and scopes it as tests. */
    override fun isForTests(): Boolean = forTests

    override fun toString(): String = if (forTests) "JuxTestSourceRoot" else "JuxSourceRoot"

    companion object {
        /** Production sources: `src/` in the §B.1 layout. */
        @JvmField
        val SOURCE = JuxSourceRootType(false)

        /** Test sources: `test/` in the §B.1 layout. */
        @JvmField
        val TEST_SOURCE = JuxSourceRootType(true)

        /** The id each kind is saved under in the `.iml`; never change these. */
        const val SOURCE_ID = "jux-source"
        const val TEST_SOURCE_ID = "jux-test-source"
    }
}

/**
 * Registers the Jux root kinds with the project model's serializer, so a
 * marked root survives a restart. Found through
 * `META-INF/services/org.jetbrains.jps.model.serialization.JpsModelSerializerExtension`
 * (the JPS model loads it with a service loader over the plugins that declare
 * `<jps.plugin/>` in `plugin.xml`).
 */
class JuxJpsModelSerializerExtension : JpsModelSerializerExtension() {
    override fun getModuleSourceRootPropertiesSerializers(): List<JpsModuleSourceRootPropertiesSerializer<*>> = listOf(
        JpsModuleSourceRootDummyPropertiesSerializer(JuxSourceRootType.SOURCE, JuxSourceRootType.SOURCE_ID),
        JpsModuleSourceRootDummyPropertiesSerializer(JuxSourceRootType.TEST_SOURCE, JuxSourceRootType.TEST_SOURCE_ID),
    )
}

/**
 * How a Jux root looks in Project Structure and the Project view: the same
 * blue and green folders Java's roots use, so a Java developer reads them at a
 * glance, under Jux's own names.
 */
abstract class JuxSourceRootEditHandlerBase(type: JuxSourceRootType) : ModuleSourceRootEditHandler<JpsDummyElement>(type) {
    override fun getFolderUnderRootIcon(): Icon? = null
    override fun getMarkRootShortcutSet(): CustomShortcutSet? = null
}

/** Project Structure presentation of [JuxSourceRootType.SOURCE]. */
class JuxSourceRootEditHandler : JuxSourceRootEditHandlerBase(JuxSourceRootType.SOURCE) {
    override fun getRootTypeName(): String = "Jux Sources"
    override fun getRootIcon(): Icon = AllIcons.Modules.SourceRoot
    override fun getRootsGroupTitle(): String = "Jux Source Folders"
    override fun getRootsGroupColor(): Color = JBColor(Color(0x0A50A1), Color(0x5092E4))
    override fun getUnmarkRootButtonText(): String = "Unmark Jux Source"
}

/** Project Structure presentation of [JuxSourceRootType.TEST_SOURCE]. */
class JuxTestSourceRootEditHandler : JuxSourceRootEditHandlerBase(JuxSourceRootType.TEST_SOURCE) {
    override fun getRootTypeName(): String = "Jux Test Sources"
    override fun getRootIcon(): Icon = AllIcons.Modules.TestRoot
    override fun getRootsGroupTitle(): String = "Jux Test Source Folders"
    override fun getRootsGroupColor(): Color = JBColor(Color(0x008C2E), Color(0x499C54))
    override fun getUnmarkRootButtonText(): String = "Unmark Jux Test Source"
}

/** Project view: right-click a directory, **Mark Directory as | Jux Sources Root**. */
class JuxMarkSourcesRootAction : MarkSourceRootAction(JuxSourceRootType.SOURCE)

/** Project view: right-click a directory, **Mark Directory as | Jux Test Sources Root**. */
class JuxMarkTestSourcesRootAction : MarkSourceRootAction(JuxSourceRootType.TEST_SOURCE)
