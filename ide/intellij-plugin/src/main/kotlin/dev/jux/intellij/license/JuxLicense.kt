package dev.jux.intellij.license

import com.intellij.ide.util.PropertiesComponent

/**
 * The plugin's "AS IS / no liability" agreement and its one-time acceptance
 * state. Acceptance is stored at the APPLICATION level ([PropertiesComponent.getInstance]
 * with no project), so the user agrees once per IDE installation, not per
 * project. The stored value is the accepted [VERSION] — bumping it re-prompts
 * everyone if the terms ever change.
 */
object JuxLicense {
    /** Accepted-terms version. Bump to force re-acceptance after a wording change. */
    const val VERSION = "1"

    private const val KEY = "dev.jux.license.acceptedVersion"

    const val TITLE = "Jux Language Plugin — License & Disclaimer"

    /** The agreement shown in the dialog (and mirrored in the plugin description). */
    val TEXT = """
        Jux Language Plugin & Toolchain — License and Disclaimer

        Provided by XTREME SOFTWARE SOLUTIONS
        Website: https://juxlang.dev/
        Source:  https://github.com/xdsswar/juxlang

        This software is provided FREE OF CHARGE and "AS IS", WITHOUT WARRANTY OF
        ANY KIND, express or implied, including but not limited to the warranties
        of merchantability, fitness for a particular purpose, and noninfringement.

        You use this software entirely at your own risk. To the maximum extent
        permitted by applicable law, in no event shall the authors, copyright
        holders, or XTREME SOFTWARE SOLUTIONS be liable for any claim, damages,
        data loss, or other liability — whether in an action of contract, tort,
        or otherwise — arising from, out of, or in connection with this software
        or the use of it.

        Jux is pre-release software: its behavior, syntax, and APIs may change at
        any time without notice, and it may contain defects.

        By clicking "I Agree" you acknowledge that you have read, understood, and
        accept these terms. If you do not agree, click "I Decline" and do not use
        the plugin.
    """.trimIndent()

    /** True once the current [VERSION] of the agreement has been accepted. */
    fun isAccepted(): Boolean = PropertiesComponent.getInstance().getValue(KEY) == VERSION

    /** Record acceptance of the current [VERSION]. */
    fun accept() = PropertiesComponent.getInstance().setValue(KEY, VERSION)
}
