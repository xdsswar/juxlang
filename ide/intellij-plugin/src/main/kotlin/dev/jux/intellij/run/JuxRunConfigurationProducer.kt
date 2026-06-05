package dev.jux.intellij.run

import com.intellij.execution.actions.ConfigurationContext
import com.intellij.execution.actions.LazyRunConfigurationProducer
import com.intellij.execution.configurations.ConfigurationFactory
import com.intellij.execution.configurations.ConfigurationTypeUtil
import com.intellij.openapi.util.Ref
import com.intellij.psi.PsiElement
import dev.jux.intellij.JuxFileType

/**
 * Makes a `.jux` file with a `main` runnable from context — right-click → Run,
 * Ctrl+Shift+F10, and (after the first run) the toolbar Run button at the top.
 *
 * "Autodetect main": [setupConfigurationFromContext] only produces a config
 * when [JuxMainDetector] finds a `main` in the file, so non-entry files don't
 * get a spurious Run option. Every step is null-guarded and wrapped so a
 * malformed context can never surface as an IDE error.
 */
class JuxRunConfigurationProducer : LazyRunConfigurationProducer<JuxRunConfiguration>() {

    override fun getConfigurationFactory(): ConfigurationFactory =
        ConfigurationTypeUtil.findConfigurationType(JuxRunConfigurationType::class.java)
            .configurationFactories[0]

    override fun setupConfigurationFromContext(
        configuration: JuxRunConfiguration,
        context: ConfigurationContext,
        sourceElement: Ref<PsiElement>,
    ): Boolean {
        return try {
            val psiFile = context.psiLocation?.containingFile ?: return false
            val vf = psiFile.virtualFile ?: return false
            if (vf.fileType != JuxFileType) return false
            val text = psiFile.text ?: return false
            if (!JuxMainDetector.hasMain(text)) return false
            configuration.filePath = vf.path
            configuration.name = vf.nameWithoutExtension
            true
        } catch (_: Exception) {
            false
        }
    }

    override fun isConfigurationFromContext(
        configuration: JuxRunConfiguration,
        context: ConfigurationContext,
    ): Boolean {
        return try {
            val vf = context.psiLocation?.containingFile?.virtualFile ?: return false
            if (vf.fileType != JuxFileType) return false
            configuration.filePath == vf.path
        } catch (_: Exception) {
            false
        }
    }
}
