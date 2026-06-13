package dev.jux.intellij.license

import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.project.Project
import com.intellij.openapi.startup.ProjectActivity

/**
 * Shows the one-time [JuxLicenseDialog] the first time a project is opened with
 * this plugin installed, until the user accepts. Acceptance is application-wide
 * ([JuxLicense]), so it appears once per IDE install — not per project.
 *
 * Declining simply doesn't persist acceptance: the agreement reappears next
 * time a Jux-capable project is opened. The dialog is modal and must run on the
 * EDT, so we hop there with [invokeLater] and re-check acceptance inside (a
 * second project opening concurrently could have accepted in the meantime).
 * Skipped entirely in unit-test mode.
 */
class JuxLicenseStartup : ProjectActivity {
    override suspend fun execute(project: Project) {
        if (JuxLicense.isAccepted()) return
        val app = ApplicationManager.getApplication()
        if (app.isUnitTestMode || app.isHeadlessEnvironment) return
        app.invokeLater {
            if (JuxLicense.isAccepted() || project.isDisposed) return@invokeLater
            if (JuxLicenseDialog().showAndGet()) {
                JuxLicense.accept()
            }
        }
    }
}
