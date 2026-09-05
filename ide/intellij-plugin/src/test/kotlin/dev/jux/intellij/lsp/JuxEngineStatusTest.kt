package dev.jux.intellij.lsp

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.lsp4ij.JuxLsp4ijStatus

/**
 * The engine gate and the widget that reports it.
 *
 * The bug this guards: `isServing` treated "LSP4IJ installed + toggle on +
 * binary resolves" as serving, without asking whether a session existed. Both
 * the fallback completion and the `juxc --check` annotator stand down on that
 * flag, so a crashed server left the editor with no completion and no
 * diagnostics — and nothing on screen said why.
 */
class JuxEngineStatusTest : BasePlatformTestCase() {

    /**
     * Under the test fixture no server is ever started, so the gate must report
     * the fallback — which is also what keeps every other test exercising the
     * IDE-side path rather than a half-started server.
     */
    fun testFixtureReportsTheFallbackEngine() {
        assertFalse(JuxLspState.isServing(project))
        assertEquals(JuxLspState.Engine.FALLBACK, JuxLspState.engine(project))
    }

    /**
     * The liveness probe must answer — not throw — on an IDE where LSP4IJ is
     * absent or the server was never started. It is called from the completion
     * gate on every keystroke, so an exception here would take the popup with
     * it.
     */
    fun testLsp4ijLivenessIsSafeWithoutAServer() {
        assertFalse(JuxLsp4ijStatus.isActive(project))
    }

    /** The widget must be constructible and silent with no Jux file open. */
    fun testWidgetIsSilentWithoutAJuxFile() {
        // The widget is its own TextPresentation (see the class declaration),
        // so the interface methods are called directly.
        val widget = JuxEngineStatusBarWidget(project)
        try {
            assertEquals("", widget.getText())
            assertNotNull(widget.getTooltipText())
        } finally {
            widget.dispose()
        }
    }

    /** The factory's id must match the `plugin.xml` registration exactly. */
    fun testFactoryIdMatchesRegistration() {
        assertEquals("JuxEngineStatus", JuxEngineStatusBarWidgetFactory().id)
    }
}
