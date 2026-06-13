package dev.jux.intellij.run

import com.intellij.execution.lineMarker.ExecutorAction
import com.intellij.execution.lineMarker.RunLineMarkerContributor
import com.intellij.icons.AllIcons
import com.intellij.psi.PsiElement
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.psi.JuxMethodDeclaration

/**
 * The green ▶ gutter icon next to a runnable `main` — the always-visible
 * counterpart to the context-menu Run that [JuxRunConfigurationProducer]
 * already provides. Clicking it offers the standard Run/Debug executor
 * actions, which flow through the same producer.
 *
 * Detection mirrors the producer: the identifier must be the **name leaf** of
 * a method declaration called `main`, in a file [JuxMainDetector] accepts
 * (so the icon and the run option always agree).
 */
class JuxRunLineMarkerContributor : RunLineMarkerContributor() {
    override fun getInfo(element: PsiElement): Info? {
        // Cheapest checks first: a method-declaration name leaf.
        if (element.elementType !== JuxTokenTypes.IDENTIFIER) return null
        val method = element.parent as? JuxMethodDeclaration ?: return null
        if (method.nameIdentifier !== element) return null

        // §TS test function: ▶ that runs `jux test <pkg.fn>` through
        // JuxTestRunConfigurationProducer (the caret context is the function).
        if (JuxTestDetector.isTestFunction(method)) {
            val qualified = JuxTestDetector.qualifiedName(method)
            return Info(
                AllIcons.RunConfigurations.TestState.Run,
                ExecutorAction.getActions(0),
            ) { "Run test '$qualified'" }
        }

        if (element.text != "main") return null
        // Authoritative signature gate (modifiers + void/int return).
        val fileText = element.containingFile?.text ?: return null
        if (!JuxMainDetector.hasMain(fileText)) return null

        return Info(
            AllIcons.RunConfigurations.TestState.Run,
            ExecutorAction.getActions(0),
        ) { "Run 'main'" }
    }
}
