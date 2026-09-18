package dev.jux.intellij.lsp

import com.intellij.openapi.project.Project
import com.intellij.openapi.roots.ModuleRootEvent
import com.intellij.openapi.roots.ModuleRootListener
import com.intellij.platform.lsp.api.LspServerManager
import dev.jux.intellij.project.JuxSourceRootsConfig
import org.eclipse.lsp4j.DidChangeConfigurationParams

/**
 * Tells a running `juxc-lsp` (native client) that the package roots changed:
 * after a directory is marked or unmarked, the server gets
 * `workspace/didChangeConfiguration` carrying the new `jux.sourceRoots`
 * (§I.4). A server that pulls with `workspace/configuration` gets the same
 * list from [JuxLspDescriptor.getWorkspaceConfiguration].
 *
 * Referenced only from `lsp.xml`, like the rest of this package.
 */
class JuxLspRootsListener(private val project: Project) : ModuleRootListener {
    override fun rootsChanged(event: ModuleRootEvent) {
        if (project.isDisposed) return
        val servers = try {
            LspServerManager.getInstance(project).getServersForProvider(JuxLspServerSupportProvider::class.java)
        } catch (_: Throwable) {
            return
        }
        if (servers.isEmpty()) return
        val settings = mapOf("jux" to mapOf("sourceRoots" to JuxSourceRootsConfig.compute(project)))
        for (server in servers) {
            server.sendNotification { it.workspaceService.didChangeConfiguration(DidChangeConfigurationParams(settings)) }
        }
    }
}
