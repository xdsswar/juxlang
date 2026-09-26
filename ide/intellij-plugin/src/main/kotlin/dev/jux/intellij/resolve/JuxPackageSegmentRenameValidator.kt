package dev.jux.intellij.resolve

import com.intellij.openapi.project.Project
import com.intellij.patterns.ElementPattern
import com.intellij.patterns.PatternCondition
import com.intellij.patterns.PlatformPatterns
import com.intellij.psi.PsiDirectory
import com.intellij.psi.PsiElement
import com.intellij.refactoring.rename.RenameInputValidatorEx
import com.intellij.util.ProcessingContext
import dev.jux.intellij.JuxPackageResolver

/**
 * Rename for a directory that IS a Jux package segment: one below a package
 * root the Jux layout vouches for (a marked root, or `src/`/`test/` under a
 * `jux.toml`, see [JuxPackageResolver]).
 *
 * A declaration's new name goes through [JuxNamesValidator], which refuses
 * every keyword, and that is right for a declaration. A package segment
 * follows ERRATA E78 instead: after the first segment a keyword is a name, so
 * renaming `src/demo/model/` to `src/demo/type/` gives the legal
 * `package demo.type;`. [JuxNamesValidator.isPackageSegment] holds the rule;
 * this only works out whether the directory is the path's first segment.
 */
class JuxPackageSegmentRenameValidator : RenameInputValidatorEx {

    override fun getPattern(): ElementPattern<out PsiElement> =
        PlatformPatterns.psiElement(PsiDirectory::class.java).with(
            object : PatternCondition<PsiDirectory>("juxPackageSegment") {
                override fun accepts(dir: PsiDirectory, context: ProcessingContext?): Boolean =
                    packageOf(dir) != null
            },
        )

    override fun isInputValid(newName: String, element: PsiElement, context: ProcessingContext): Boolean {
        val pkg = packageOf(element as? PsiDirectory ?: return false) ?: return false
        return JuxNamesValidator.isPackageSegment(newName, first = '.' !in pkg)
    }

    override fun getErrorMessage(newName: String, project: Project): String =
        if (newName in JuxNamesValidator.UNNAMEABLE_SEGMENTS) {
            "'$newName' cannot be a package segment: the Rust path could not name it (ERRATA E78)"
        } else {
            "'$newName' is not a valid package segment (a keyword may only follow the first segment)"
        }

    private companion object {
        /** The dotted package [dir] stands for, or null when it is no Jux package directory. */
        fun packageOf(dir: PsiDirectory): String? {
            val root = JuxPackageResolver.rootFor(dir.virtualFile, dir.project)?.takeIf { it.authoritative } ?: return null
            return JuxPackageResolver.packageUnder(root.dir, dir.virtualFile)?.takeIf { it.isNotEmpty() }
        }
    }
}
