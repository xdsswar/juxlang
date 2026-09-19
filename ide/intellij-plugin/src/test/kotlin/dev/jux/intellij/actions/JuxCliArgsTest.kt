package dev.jux.intellij.actions

import dev.jux.intellij.actions.JuxCliArgs.GitRef
import dev.jux.intellij.actions.JuxCliArgs.NewKind
import dev.jux.intellij.actions.JuxCliArgs.Source
import junit.framework.TestCase

/**
 * The exact `jux` command lines behind the Tools | Jux actions
 * (JUX-BUILD-SYSTEM-ADDENDUM §B.10.5, §B.14, §B.15).
 */
class JuxCliArgsTest : TestCase() {

    fun testNewKinds() {
        assertEquals(listOf("new", "app"), JuxCliArgs.new(" app ", NewKind.BINARY))
        assertEquals(listOf("new", "--lib", "geo"), JuxCliArgs.new("geo", NewKind.LIBRARY))
        assertEquals(listOf("new", "--workspace", "mono"), JuxCliArgs.new("mono", NewKind.WORKSPACE))
    }

    fun testAddFromTheRegistryWithAVersion() {
        assertEquals(listOf("add", "com.x.json@1.0"), JuxCliArgs.add("com.x.json", version = "1.0"))
        assertEquals(listOf("add", "rust.rand"), JuxCliArgs.add("rust.rand"))
    }

    fun testAddAPathDependencyDropsTheVersion() {
        assertEquals(
            listOf("add", "geo", "--path", "../geo"),
            JuxCliArgs.add("geo", version = "1.0", source = Source.PATH, location = "../geo"),
        )
    }

    fun testAddAGitDependencyWithARefAndFeatures() {
        assertEquals(
            listOf("add", "com.acme.json", "--git", "https://github.com/acme/json", "--tag", "v1.4.2", "--features", "fast,serde"),
            JuxCliArgs.add(
                "com.acme.json",
                source = Source.GIT,
                location = "https://github.com/acme/json",
                gitRef = GitRef.TAG,
                refValue = "v1.4.2",
                features = " fast , serde ,",
            ),
        )
    }

    fun testABlankRefLeavesTheRefOut() {
        assertEquals(
            listOf("add", "x", "--git", "https://h/x"),
            JuxCliArgs.add("x", source = Source.GIT, location = "https://h/x", gitRef = GitRef.BRANCH, refValue = " "),
        )
    }

    fun testRemoveAndDoc() {
        assertEquals(listOf("remove", "com.x.json"), JuxCliArgs.remove("com.x.json"))
        assertEquals(listOf("doc"), JuxCliArgs.doc(open = false))
        assertEquals(listOf("doc", "--open"), JuxCliArgs.doc(open = true))
    }
}
