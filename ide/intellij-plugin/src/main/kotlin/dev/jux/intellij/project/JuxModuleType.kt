package dev.jux.intellij.project

import com.intellij.openapi.module.ModuleType
import com.intellij.openapi.module.ModuleTypeManager
import dev.jux.intellij.JuxIcons
import javax.swing.Icon

/**
 * The Jux module/project type — what the New Project wizard offers as a "Jux"
 * generator. Scaffolding is done by [JuxModuleBuilder].
 */
class JuxModuleType : ModuleType<JuxModuleBuilder>(ID) {
    override fun createModuleBuilder(): JuxModuleBuilder = JuxModuleBuilder()
    override fun getName(): String = "Jux"
    override fun getDescription(): String =
        "Jux project: a jux.toml manifest with a src/ source root and a starter main.jux."
    override fun getNodeIcon(isOpened: Boolean): Icon = JuxIcons.FILE

    companion object {
        const val ID = "JUX_MODULE_TYPE"

        /** The registered singleton, resolved through the platform. */
        val instance: JuxModuleType
            get() = ModuleTypeManager.getInstance().findByID(ID) as JuxModuleType
    }
}
