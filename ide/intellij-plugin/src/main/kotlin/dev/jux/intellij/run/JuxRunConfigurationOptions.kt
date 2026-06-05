package dev.jux.intellij.run

import com.intellij.execution.configurations.RunConfigurationOptions

/**
 * Persisted state for a Jux run configuration: which file to run and which
 * `juxc` executable to use. Stored via the platform's `StoredProperty`
 * mechanism so it round-trips through the run-configuration XML automatically.
 */
class JuxRunConfigurationOptions : RunConfigurationOptions() {
    private val filePathProp = string("").provideDelegate(this, "juxFilePath")
    private val juxcPathProp = string("juxc").provideDelegate(this, "juxcPath")

    var filePath: String
        get() = filePathProp.getValue(this) ?: ""
        set(value) = filePathProp.setValue(this, value)

    var juxcPath: String
        get() = juxcPathProp.getValue(this) ?: "juxc"
        set(value) = juxcPathProp.setValue(this, value)
}
