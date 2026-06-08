package dev.jux.intellij.psi

import com.intellij.extapi.psi.PsiFileBase
import com.intellij.openapi.fileTypes.FileType
import com.intellij.psi.FileViewProvider
import com.intellij.psi.tree.IFileElementType
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.JuxLanguage

/** The root node type for a parsed `.jux` file. */
val JUX_FILE: IFileElementType = IFileElementType("JUX_FILE", JuxLanguage)

/**
 * The PSI root for a `.jux` source file. Backed by [JuxParserDefinition], which
 * resolves the lexer + parser that build the tree under this node.
 */
class JuxFile(viewProvider: FileViewProvider) : PsiFileBase(viewProvider, JuxLanguage) {
    override fun getFileType(): FileType = JuxFileType
    override fun toString(): String = "Jux File"
}
