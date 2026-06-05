package dev.jux.intellij.run

import com.intellij.execution.configurations.ConfigurationFactory
import com.intellij.execution.configurations.ConfigurationType
import com.intellij.execution.configurations.ConfigurationTypeBase
import com.intellij.execution.configurations.RunConfiguration
import com.intellij.openapi.project.Project
import com.intellij.openapi.util.NotNullLazyValue
import dev.jux.intellij.JuxIcons

/**
 * The "Jux" run-configuration type — one entry in Run/Debug Configurations
 * that runs a `.jux` file via `juxc <file> --run`.
 */
class JuxRunConfigurationType : ConfigurationTypeBase(
    ID,
    "Jux",
    "Run a Jux file with juxc",
    NotNullLazyValue.createValue { JuxIcons.FILE },
) {
    init {
        addFactory(JuxConfigurationFactory(this))
    }

    companion object {
        const val ID = "JuxRunConfiguration"
    }
}

/** Factory producing [JuxRunConfiguration]s and their persisted options. */
class JuxConfigurationFactory(type: ConfigurationType) : ConfigurationFactory(type) {
    override fun getId(): String = JuxRunConfigurationType.ID

    override fun createTemplateConfiguration(project: Project): RunConfiguration =
        JuxRunConfiguration(project, this, "Jux")

    override fun getOptionsClass(): Class<JuxRunConfigurationOptions> =
        JuxRunConfigurationOptions::class.java
}
