package dev.jux.intellij.folding

import com.intellij.application.options.editor.CodeFoldingOptionsProvider
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.openapi.options.BeanConfigurable
import com.intellij.util.xmlb.XmlSerializerUtil

/**
 * Which Jux fold regions start collapsed, beside the general ones every
 * language shares (imports, file header, method bodies, documentation
 * comments, in Settings | Editor | General | Code Folding). The defaults are
 * Java's: everything here starts expanded.
 */
@State(name = "JuxCodeFoldingSettings", storages = [Storage("editor.xml")])
class JuxCodeFoldingSettings : PersistentStateComponent<JuxCodeFoldingSettings> {
    /** `{ get; set; }` accessor lists, as Java's "Simple property accessors". */
    var collapseAccessors: Boolean = false

    /** The bodies of types declared inside another type. */
    var collapseInnerClasses: Boolean = false

    /** Block-bodied lambdas, `() -> { ... }`. */
    var collapseLambdas: Boolean = false

    /** Runs of two or more `//` comment lines. */
    var collapseEndOfLineComments: Boolean = false

    /** `/* ... */` comments spanning several lines. */
    var collapseMultilineComments: Boolean = false

    /** String literals too long to read at a glance, and multi-line raw strings. */
    var collapseLongStrings: Boolean = false

    override fun getState(): JuxCodeFoldingSettings = this

    override fun loadState(state: JuxCodeFoldingSettings) {
        XmlSerializerUtil.copyBean(state, this)
    }

    companion object {
        fun getInstance(): JuxCodeFoldingSettings =
            ApplicationManager.getApplication().getService(JuxCodeFoldingSettings::class.java)
    }
}

/** The "Jux" group of Settings | Editor | General | Code Folding, beside Java's. */
class JuxCodeFoldingOptionsProvider :
    BeanConfigurable<JuxCodeFoldingSettings>(JuxCodeFoldingSettings.getInstance(), "Jux"),
    CodeFoldingOptionsProvider {
    init {
        val s = instance!!
        checkBox("Simple property accessors", s::collapseAccessors)
        checkBox("Inner classes", s::collapseInnerClasses)
        checkBox("Lambda bodies", s::collapseLambdas)
        checkBox("End of line comments sequences", s::collapseEndOfLineComments)
        checkBox("Multiline comments", s::collapseMultilineComments)
        checkBox("Long string literals", s::collapseLongStrings)
    }
}
