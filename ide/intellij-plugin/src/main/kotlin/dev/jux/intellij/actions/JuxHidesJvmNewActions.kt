package dev.jux.intellij.actions

import com.intellij.openapi.actionSystem.ActionManager
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.AnActionWrapper
import com.intellij.openapi.actionSystem.impl.ActionConfigurationCustomizer

/**
 * **New → Java Class** and its siblings, hidden inside a Jux module.
 *
 * A Jux project has no use for them, and in a directory full of `.jux` files
 * they are the entries nearest the cursor. They stay exactly as they were
 * everywhere else: this wraps each one rather than unregistering it, so a Java
 * or Kotlin project in the same IDE is untouched, and so is a directory in
 * this project that is not under a `jux.toml`.
 *
 * Every id is optional. The plugin depends only on
 * `com.intellij.modules.platform`, so in CLion or GoLand none of these exist,
 * and a missing one is simply skipped.
 */
internal class JuxHidesJvmNewActions : ActionConfigurationCustomizer {

    override fun customize(actionManager: ActionManager) {
        for (id in HIDDEN_IN_JUX_MODULES) {
            val existing = actionManager.getAction(id) ?: continue
            if (existing is HiddenInJuxModule) continue
            actionManager.replaceAction(id, HiddenInJuxModule(existing))
        }
    }

    private companion object {
        /**
         * The "new JVM source file" entries. Kept as an explicit list rather
         * than a pattern over the New group: hiding an entry the user still
         * wants is worse than leaving one they do not.
         */
        val HIDDEN_IN_JUX_MODULES = listOf(
            "NewClass",          // Java: Class / Interface / Enum / Record
            "NewJavaFile",
            "Kotlin.NewFile",
            "NewGroovyClass",
            "Scala.NewClass",
            "NewModuleInfo",     // Java: module-info.java
            "NewPackageInfo",    // Java: package-info.java
        )
    }
}

/**
 * Delegates entirely to the action it wraps, except that it makes itself
 * invisible when the target directory is inside a Jux module.
 */
private class HiddenInJuxModule(private val delegate: AnAction) : AnActionWrapper(delegate) {

    override fun update(e: AnActionEvent) {
        super.update(e)
        // Only ever REMOVES the entry. Whatever the wrapped action decided
        // about being enabled or visible stands everywhere else.
        if (e.presentation.isVisible && JuxProjectContext.inJuxModule(e)) {
            e.presentation.isEnabledAndVisible = false
        }
    }
}
