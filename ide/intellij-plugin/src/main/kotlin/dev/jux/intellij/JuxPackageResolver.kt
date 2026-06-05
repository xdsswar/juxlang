package dev.jux.intellij

import com.intellij.openapi.project.Project
import com.intellij.openapi.roots.ProjectFileIndex
import com.intellij.openapi.vfs.VirtualFile

/**
 * Derives a Jux `package` name from a directory's path relative to its source
 * root — exactly how the Java plugin treats `.java` files (§I.4).
 *
 * Resolution order for the root:
 *  1. The nearest **source root** the IDE knows about (`ProjectFileIndex`).
 *  2. Failing that, the module/project **content root**.
 *
 * The dotted package is the root-relative path with separators replaced by
 * `.`. A file sitting directly in the root yields an empty string, and the
 * templates omit the `package` line in that case.
 */
object JuxPackageResolver {
    /** Infer the package for a file/dir, or `null` when no root contains it. */
    fun inferPackage(file: VirtualFile, project: Project): String? {
        val index = ProjectFileIndex.getInstance(project)
        val dir = if (file.isDirectory) file else file.parent ?: return null

        val root = index.getSourceRootForFile(dir)
            ?: index.getContentRootForFile(dir)
            ?: return null

        val relative = com.intellij.openapi.vfs.VfsUtilCore.getRelativePath(dir, root, '/')
            ?: return ""

        if (relative.isEmpty()) return ""
        return relative.replace('/', '.')
    }
}
