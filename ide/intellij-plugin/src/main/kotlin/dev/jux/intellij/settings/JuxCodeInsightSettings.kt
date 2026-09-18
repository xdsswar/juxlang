package dev.jux.intellij.settings

import com.intellij.application.options.editor.AutoImportOptionsProvider
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.openapi.components.StoragePathMacros
import com.intellij.openapi.options.BeanConfigurable
import com.intellij.openapi.project.Project
import com.intellij.util.xmlb.XmlSerializerUtil

/**
 * The Jux half of **Settings | Editor | General | Auto Import**, kept apart
 * from Java's the way Kotlin keeps its own: turning Jux's auto-import on does
 * not change how the Java editor behaves, and the options exist in IDEs that
 * have no Java plugin at all.
 *
 * "Add unambiguous imports" is an editor habit, so it follows the user from
 * project to project (application level). "Optimize imports on the fly"
 * rewrites files, so it is decided per project (workspace level) -- the same
 * split Java and Kotlin make.
 */
@Service(Service.Level.APP)
@State(name = "JuxCodeInsightSettings", storages = [Storage("jux.xml")])
class JuxCodeInsightSettings : PersistentStateComponent<JuxCodeInsightSettings.State> {
    class State {
        /** Write an `import` as soon as a type with exactly one candidate is typed. */
        @JvmField
        var addUnambiguousImportsOnTheFly: Boolean = false
    }

    private var state = State()

    override fun getState(): State = state
    override fun loadState(s: State) = XmlSerializerUtil.copyBean(s, state)

    var addUnambiguousImportsOnTheFly: Boolean
        get() = state.addUnambiguousImportsOnTheFly
        set(value) {
            state.addUnambiguousImportsOnTheFly = value
        }

    companion object {
        fun getInstance(): JuxCodeInsightSettings =
            ApplicationManager.getApplication().getService(JuxCodeInsightSettings::class.java)
    }
}

/** The per-project half: whether Jux files are optimized as they are edited. */
@Service(Service.Level.PROJECT)
@State(name = "JuxCodeInsightWorkspaceSettings", storages = [Storage(StoragePathMacros.WORKSPACE_FILE)])
class JuxCodeInsightWorkspaceSettings : PersistentStateComponent<JuxCodeInsightWorkspaceSettings.State> {
    class State {
        /** Remove unused imports once highlighting finishes, as Java's option does. */
        @JvmField
        var optimizeImportsOnTheFly: Boolean = false
    }

    private var state = State()

    override fun getState(): State = state
    override fun loadState(s: State) = XmlSerializerUtil.copyBean(s, state)

    var optimizeImportsOnTheFly: Boolean
        get() = state.optimizeImportsOnTheFly
        set(value) {
            state.optimizeImportsOnTheFly = value
        }

    companion object {
        fun getInstance(project: Project): JuxCodeInsightWorkspaceSettings =
            project.getService(JuxCodeInsightWorkspaceSettings::class.java)
    }
}

/** The "Jux" group on the Auto Import page. */
class JuxAutoImportOptionsProvider(project: Project) :
    BeanConfigurable<JuxCodeInsightWorkspaceSettings>(JuxCodeInsightWorkspaceSettings.getInstance(project), "Jux"),
    AutoImportOptionsProvider {

    init {
        val app = JuxCodeInsightSettings.getInstance()
        checkBox("Add unambiguous imports on the fly", app::addUnambiguousImportsOnTheFly)
        checkBox("Optimize imports on the fly", instance::optimizeImportsOnTheFly)
    }
}
