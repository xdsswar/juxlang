package dev.jux.intellij.run

import com.intellij.execution.actions.ConfigurationContext
import com.intellij.execution.actions.ConfigurationFromContext
import com.intellij.execution.actions.LazyRunConfigurationProducer
import com.intellij.execution.configurations.ConfigurationFactory
import com.intellij.execution.configurations.ConfigurationTypeUtil
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.util.Ref
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.psi.JuxMethodDeclaration

/**
 * Produces **test-mode** [JuxRunConfiguration]s from context (§TS.2):
 *
 *  - caret on/inside a `@Test` free function → run just that test
 *    (pattern = its package-qualified name, §TS.8 substring filter);
 *  - a `.jux` file containing tests → run the file's tests
 *    (pattern = its package — best the runner's substring filter can scope);
 *  - a directory → run all tests in the project.
 *
 * Shares the run producer's configuration type/factory; the two never claim
 * each other's configs because [isConfigurationFromContext] keys on the mode.
 */
class JuxTestRunConfigurationProducer : LazyRunConfigurationProducer<JuxRunConfiguration>() {

    override fun getConfigurationFactory(): ConfigurationFactory =
        ConfigurationTypeUtil.findConfigurationType(JuxRunConfigurationType::class.java)
            .configurationFactories[0]

    override fun setupConfigurationFromContext(
        configuration: JuxRunConfiguration,
        context: ConfigurationContext,
        sourceElement: Ref<PsiElement>,
    ): Boolean {
        return try {
            configuration.mode = JuxRunConfiguration.MODE_TEST

            // Directory: run every test under the manifest.
            val vf = context.psiLocation?.containingFile?.virtualFile
                ?: context.dataContext.getData(CommonDataKeys.VIRTUAL_FILE)
                ?: return false
            if (vf.isDirectory) {
                configuration.filePath = vf.path
                configuration.testPattern = ""
                configuration.name = "All tests in ${vf.name}"
                // Only offer it when a manifest is actually reachable.
                return configuration.manifestRoot() != null
            }
            if (vf.fileType != JuxFileType) return false

            val psiFile = context.psiLocation?.containingFile ?: return false

            // Caret inside a @Test free function → that one test.
            val method = PsiTreeUtil.getParentOfType(
                context.psiLocation, JuxMethodDeclaration::class.java, false,
            )
            if (method != null && JuxTestDetector.isTestFunction(method)) {
                val qualified = JuxTestDetector.qualifiedName(method)
                if (qualified.isEmpty()) return false
                configuration.filePath = vf.path
                configuration.testPattern = qualified
                configuration.name = qualified
                sourceElement.set(method)
                return true
            }

            // The file itself, when it has tests → its package's tests.
            if (!JuxTestDetector.hasTestsText(psiFile.text) || !JuxTestDetector.hasTests(psiFile)) {
                return false
            }
            configuration.filePath = vf.path
            configuration.testPattern = JuxTestDetector.packageName(psiFile)
            configuration.name = "Tests in ${vf.name}"
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
            if (!configuration.isTestMode()) return false
            // Re-derive what this context would produce and compare the keys.
            val candidate =
                configurationFactory.createTemplateConfiguration(context.project) as JuxRunConfiguration
            if (!setupConfigurationFromContext(candidate, context, Ref())) return false
            configuration.filePath == candidate.filePath &&
                configuration.testPattern == candidate.testPattern
        } catch (_: Exception) {
            false
        }
    }

    /**
     * When the caret is inside a `@Test` function, the single-test entry should
     * lead the context menu; a file holding both a `main` and tests keeps both
     * entries (this only orders them, it doesn't suppress the run config).
     */
    override fun isPreferredConfiguration(self: ConfigurationFromContext, other: ConfigurationFromContext): Boolean {
        val mine = self.configuration as? JuxRunConfiguration ?: return false
        val theirs = other.configuration as? JuxRunConfiguration ?: return true
        return mine.isTestMode() && !theirs.isTestMode()
    }
}
