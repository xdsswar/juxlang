package dev.jux.intellij.license

import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent

/**
 * Help-menu action to re-display the agreement at any time (so users can review
 * the disclaimer after the one-time prompt). View-only — it doesn't change the
 * stored acceptance.
 */
class JuxShowLicenseAction : AnAction() {
    override fun actionPerformed(e: AnActionEvent) {
        JuxLicenseDialog().show()
    }
}
