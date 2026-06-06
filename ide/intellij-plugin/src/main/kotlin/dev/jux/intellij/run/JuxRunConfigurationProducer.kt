package dev.jux.intellij.run

import com.intellij.execution.actions.ConfigurationContext
import com.intellij.execution.actions.LazyRunConfigurationProducer
import com.intellij.execution.configurations.ConfigurationFactory
import com.intellij.execution.configurations.ConfigurationTypeUtil
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.fileEditor.FileDocumentManager
import com.intellij.openapi.util.Ref
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.psi.PsiElement
import dev.jux.intellij.JuxFileType

/**
 * Makes a `.jux` file with a `main` runnable from context — right-click → Run,
 * Ctrl+Shift+F10, and (after the first run) the toolbar Run button at the top.
 *
 * "Autodetect main": [setupConfigurationFromContext] only produces a config
 * when [JuxMainDetector] finds a `main` in the file (a free `main` or a
 * `static main` in a class), so non-entry files don't get a spurious Run
 * option. Every step is null-guarded and wrapped so a malformed context can
 * never surface as an IDE error.
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
            val (vf, text) = juxFileAndText(context) ?: return false
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
            val vf = juxVirtualFile(context) ?: return false
            configuration.filePath == vf.path
        } catch (_: Exception) {
            false
        }
    }

    /** The `.jux` virtual file for this context, from PSI or the data context. */
    private fun juxVirtualFile(context: ConfigurationContext): VirtualFile? {
        val fromPsi = context.psiLocation?.containingFile?.virtualFile
        val vf = fromPsi ?: context.dataContext.getData(CommonDataKeys.VIRTUAL_FILE)
        return vf?.takeIf { it.fileType == JuxFileType }
    }

    /**
     * The context's `.jux` file plus its current text. Prefers the PSI text
     * (reflects unsaved edits); falls back to the open document, then disk.
     */
    private fun juxFileAndText(context: ConfigurationContext): Pair<VirtualFile, String>? {
        val psiFile = context.psiLocation?.containingFile
        val vf = psiFile?.virtualFile ?: context.dataContext.getData(CommonDataKeys.VIRTUAL_FILE)
        if (vf == null || vf.fileType != JuxFileType) return null
        val text = psiFile?.text
            ?: FileDocumentManager.getInstance().getDocument(vf)?.text
            ?: runCatching { String(vf.contentsToByteArray(), vf.charset) }.getOrNull()
            ?: return null
        return vf to text
    }
}
