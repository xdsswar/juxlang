package dev.jux.intellij.resolve

import com.intellij.openapi.vfs.VfsUtil
import com.intellij.openapi.vfs.VirtualFile

/**
 * The `jux.std` sources the plugin ships: the compiler's embedded standard
 * library (`Option`, `Result`, the iterator combinators, `Mutex`, the
 * exception hierarchy, ...), written out at build time by the
 * `generateJuxStd` Gradle task from `crates/juxc-driver/src/stdlib_embedded.rs`.
 *
 * Unlike the toolchain's `.jux.d` stubs ([JuxStubRoots]), which depend on
 * what is installed on the machine, these are part of the plugin and fixed
 * by the build, so they are indexed in tests as well.
 */
object JuxBundledStd {

    /** The `jux-std` resource directory (a jar entry in an installed plugin), or null. */
    fun root(): VirtualFile? =
        JuxBundledStd::class.java.getResource("/jux-std")?.let { VfsUtil.findFileByURL(it) }
}
