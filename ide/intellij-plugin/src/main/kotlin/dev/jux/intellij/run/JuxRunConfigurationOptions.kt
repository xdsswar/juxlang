package dev.jux.intellij.run

import com.intellij.execution.configurations.RunConfigurationOptions

/**
 * Persisted state for a Jux run configuration: which file to run and which
 * `juxc` executable to use, plus the §TS test-mode fields (`jux test`).
 * Stored via the platform's `StoredProperty` mechanism so it round-trips
 * through the run-configuration XML automatically.
 */
class JuxRunConfigurationOptions : RunConfigurationOptions() {
    private val filePathProp = string("").provideDelegate(this, "juxFilePath")
    private val juxcPathProp = string("juxc").provideDelegate(this, "juxcPath")

    /** `"run"` (juxc --run, the default) or `"test"` (`jux test`, §TS.2). */
    private val modeProp = string("run").provideDelegate(this, "juxMode")

    /** Substring filter for `jux test <pattern>` (§TS.8); blank = all tests. */
    private val testPatternProp = string("").provideDelegate(this, "juxTestPattern")

    /** `jux test --release` — build the test runner with optimizations. */
    private val releaseProp = property(false).provideDelegate(this, "juxTestRelease")

    var filePath: String
        get() = filePathProp.getValue(this) ?: ""
        set(value) = filePathProp.setValue(this, value)

    var juxcPath: String
        get() = juxcPathProp.getValue(this) ?: "juxc"
        set(value) = juxcPathProp.setValue(this, value)

    var mode: String
        get() = modeProp.getValue(this) ?: "run"
        set(value) = modeProp.setValue(this, value)

    var testPattern: String
        get() = testPatternProp.getValue(this) ?: ""
        set(value) = testPatternProp.setValue(this, value)

    var release: Boolean
        get() = releaseProp.getValue(this)
        set(value) = releaseProp.setValue(this, value)
}
