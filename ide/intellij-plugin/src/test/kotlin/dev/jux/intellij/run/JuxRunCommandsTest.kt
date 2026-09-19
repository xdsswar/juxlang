package dev.jux.intellij.run

import junit.framework.TestCase
import java.io.File
import java.nio.file.Files

/**
 * The command lines a Jux run configuration builds ([JuxRunCommands]), the
 * profile and example pickers ([JuxManifestInfo]), and the example-label
 * mapping of the settings editor: the build-system options of
 * JUX-BUILD-SYSTEM-ADDENDUM §B.9 / §B.1.3 exactly as `jux` spells them.
 */
class JuxRunCommandsTest : TestCase() {

    private val diag = JuxRunCommands.CONSOLE_DIAGNOSTICS

    fun testConsoleAsksForFramedColoredDiagnostics() {
        assertEquals(listOf("--diagnostic-format", "human", "--color", "always"), diag)
    }

    fun testPlainProjectRun() {
        assertEquals(listOf("run") + diag, JuxRunCommands.projectRun(example = "", profile = ""))
    }

    fun testExampleAndProfile() {
        assertEquals(
            listOf("run", "--example", "shapes", "--profile", "fast") + diag,
            JuxRunCommands.projectRun(example = " shapes ", profile = "fast"),
        )
    }

    fun testAllExamplesBuildsThemAll() {
        assertEquals(
            listOf("build", "--examples") + diag,
            JuxRunCommands.projectRun(example = JuxRunCommands.ALL_EXAMPLES, profile = ""),
        )
    }

    fun testTestModeWithPatternAndRelease() {
        assertEquals(
            listOf("test", "parse", "--release") + diag,
            JuxRunCommands.test(pattern = "parse", release = true, profile = "", doc = false),
        )
    }

    fun testProfileWinsOverReleaseBecauseTheCliRefusesBoth() {
        assertEquals(
            listOf("test", "--profile", "ci") + diag,
            JuxRunCommands.test(pattern = "", release = true, profile = "ci", doc = false),
        )
    }

    fun testDocExamplesIgnoreThePattern() {
        assertEquals(
            listOf("test", "--doc") + diag,
            JuxRunCommands.test(pattern = "parse", release = false, profile = "", doc = true),
        )
    }

    fun testStandaloneFile() {
        assertEquals(listOf("/p/a.jux", "--run") + diag, JuxRunCommands.standalone("/p/a.jux"))
    }

    fun testProfilesAreBuiltinsThenTheManifestsOwn() {
        val toml = """
            [package]
            name = "app"

            [profile.release]
            opt-level = 3

            [profile.fast]   # a custom one
            extends = "release"

            [ profile . "size-opt" ]
            opt-level = "z"
        """.trimIndent()
        assertEquals(listOf("dev", "release", "test", "bench", "fast", "size-opt"), JuxManifestInfo.profiles(toml))
    }

    fun testExamplesAreFilesAndDirectoriesWithJuxInside() {
        val root = Files.createTempDirectory("jux-ex").toFile()
        try {
            val ex = File(root, "examples").apply { mkdirs() }
            File(ex, "hello.jux").writeText("void main() {}")
            File(ex, "multi/src").mkdirs()
            File(ex, "multi/src/main.jux").writeText("void main() {}")
            File(ex, "empty").mkdirs()
            File(ex, "README.md").writeText("docs")
            File(ex, ".hidden.jux").writeText("")
            File(ex, "target").mkdirs()
            assertEquals(listOf("hello", "multi"), JuxManifestInfo.examples(root))
        } finally {
            root.deleteRecursively()
        }
    }

    fun testNoExamplesDirectory() {
        assertEquals(emptyList<String>(), JuxManifestInfo.examples(File("does-not-exist-xyz")))
    }

    fun testExampleLabelsRoundTrip() {
        for (value in listOf("", JuxRunCommands.ALL_EXAMPLES, "shapes")) {
            assertEquals(value, JuxSettingsEditor.exampleValue(JuxSettingsEditor.exampleLabel(value)))
        }
        assertEquals("", JuxSettingsEditor.exampleValue(JuxSettingsEditor.EXAMPLE_MAIN))
        assertEquals(JuxRunCommands.ALL_EXAMPLES, JuxSettingsEditor.exampleValue(JuxSettingsEditor.EXAMPLE_ALL))
        assertEquals("typed", JuxSettingsEditor.exampleValue("  typed "))
    }
}
