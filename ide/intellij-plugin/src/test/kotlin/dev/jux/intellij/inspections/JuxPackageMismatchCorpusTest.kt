package dev.jux.intellij.inspections

import com.intellij.testFramework.fixtures.BasePlatformTestCase
import dev.jux.intellij.JuxPackageResolver
import java.io.File

/**
 * Zero false positives for the package-mismatch inspection on real projects.
 *
 * The flat corpus sweep (`JuxCorpusHighlightingTest`) loads every example
 * into one plain source root, where the inspection correctly stays silent: it
 * cannot know the layout there. This test is the one that means something. It
 * copies every multi-file example PROJECT (each directory with a `jux.toml`)
 * into the fixture with its directories intact, so the manifest rule applies
 * and every file is checked against its real location. These projects build
 * with `juxc`, which enforces the same rule (`E0301`), so any report here is a
 * plugin bug.
 */
class JuxPackageMismatchCorpusTest : BasePlatformTestCase() {

    private val examples = File("../../examples")

    /** Directory names that hold build output, never hand-written Jux. */
    private val skip = setOf("target", ".rust-build", ".jux-stubs", "build", ".git")

    fun testExampleProjectsHaveNoPackageMismatches() {
        assertTrue("examples dir not found at ${examples.absolutePath}", examples.isDirectory)
        myFixture.enableInspections(JuxPackageMismatchInspection())

        val projects = examples.walkTopDown()
            .onEnter { it == examples || it.name !in skip }
            .filter { it.isFile && it.name == JuxPackageResolver.MANIFEST }
            .map { it.parentFile }
            .toList()
        assertTrue("no example projects found", projects.isNotEmpty())

        // Every file of every project first, so each file is checked with its
        // whole project present.
        val sources = ArrayList<String>()
        for (project in projects) {
            project.walkTopDown()
                .onEnter { it == project || it.name !in skip }
                .filter { it.isFile && (it.name.endsWith(".jux") || it.name == JuxPackageResolver.MANIFEST) }
                .forEach { f ->
                    val path = "corpus/" + f.relativeTo(examples).invariantSeparatorsPath
                    if (myFixture.findFileInTempDir(path) == null) {
                        myFixture.addFileToProject(path, f.readText())
                        if (f.name.endsWith(".jux")) sources += path
                    }
                }
        }

        val failures = StringBuilder()
        var checked = 0
        for (path in sources.sorted()) {
            val vFile = myFixture.findFileInTempDir(path)
            if (JuxPackageResolver.expectedPackages(vFile, project) != null) checked++
            myFixture.configureFromExistingVirtualFile(vFile)
            val reported = myFixture.doHighlighting()
                .mapNotNull { it.description }
                .filter { "does not correspond to the file location" in it || "Missing package declaration" in it }
            if (reported.isNotEmpty()) failures.appendLine("- $path: ${reported.joinToString("; ")}")
        }
        // The test only means something if the layout rule actually applied.
        assertTrue("no example file had an authoritative package root ($sources)", checked > 0)
        assertTrue("package mismatches reported on example projects that build:\n$failures", failures.isEmpty())
    }
}
