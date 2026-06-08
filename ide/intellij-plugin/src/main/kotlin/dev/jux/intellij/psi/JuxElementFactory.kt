package dev.jux.intellij.psi

import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFileFactory
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.highlight.JuxTokenTypes

/** Builds throwaway PSI fragments — used by Rename to mint identifier leaves. */
object JuxElementFactory {
    /** A standalone identifier leaf with text [name], parsed from a dummy file. */
    fun createIdentifier(project: Project, name: String): PsiElement {
        val file = PsiFileFactory.getInstance(project)
            .createFileFromText("_dummy.jux", JuxFileType, "class $name {}")
        return PsiTreeUtil.collectElements(file) { it.elementType === JuxTokenTypes.IDENTIFIER }.first()
    }
}
