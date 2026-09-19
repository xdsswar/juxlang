package dev.jux.intellij.actions

import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.DefaultActionGroup
import com.intellij.testFramework.fixtures.BasePlatformTestCase

/** Every `jux` project command is registered, under Tools | Jux. */
class JuxCliActionsRegistrationTest : BasePlatformTestCase() {

    fun testToolsJuxGroupHoldsEveryCommand() {
        val manager = ActionManager.getInstance()
        val group = manager.getAction("Jux.ToolsGroup") as DefaultActionGroup
        val ids = group.childActionsOrStubs.mapNotNull { manager.getId(it) }
        for (id in listOf(
            "Jux.Cli.New", "Jux.Cli.Init", "Jux.Cli.Add", "Jux.Cli.Remove", "Jux.Cli.Tree",
            "Jux.Cli.Doc", "Jux.Cli.DocOpen", "Jux.Cli.DocTests", "Jux.Cli.Clean",
        )) {
            assertTrue("$id in Tools | Jux: $ids", id in ids)
        }
        val tools = manager.getAction("ToolsMenu") as DefaultActionGroup
        assertTrue(tools.childActionsOrStubs.any { manager.getId(it) == "Jux.ToolsGroup" })
    }
}
