package dev.jux.intellij.psi

import com.intellij.extapi.psi.PsiFileBase
import com.intellij.lang.ASTNode
import com.intellij.lang.LanguageParserDefinitions
import com.intellij.lang.PsiBuilderFactory
import com.intellij.openapi.fileTypes.FileType
import com.intellij.psi.FileViewProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IFileElementType
import dev.jux.intellij.JuxFileType
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.parser.markForeignStub

/**
 * The file-name suffix of a generated **foreign declaration stub**.
 *
 * `Path.extension` would only see `d`, so stub-ness is decided on the full
 * suffix: `image.jux.d` is a stub, a stray `image.d` is not. Mirrors the
 * compiler's `juxc_driver::stubs::is_stub_path`.
 */
const val JUX_STUB_SUFFIX: String = ".jux.d"

/** True when [name] is a generated foreign declaration stub's file name. */
fun isJuxStubFileName(name: String?): Boolean = name != null && name.endsWith(JUX_STUB_SUFFIX)

/**
 * The root node type for a parsed Jux file, which also decides whether the
 * parse runs in **foreign-stub mode**.
 *
 * A `.jux.d` stub is machine-written from a Rust crate's API, so it declares
 * members with names Jux reserves: `NonNull.new`, `Stdio.null`, `Shader.new`.
 * The compiler accepts those (`Parser::parse_decl_name` takes any keyword and
 * the `null`/`true`/`false` literal tokens), and the plugin has to agree, or
 * the parse of the BUNDLED `rust-std.jux.d` dies at the first
 * `public static NonNull? new(T* ptr);` and everything declared after it stops
 * being indexed. That was one of the two causes of imports flickering
 * "unresolved import" while quick-doc on the same word resolved fine.
 *
 * The flag has to reach the parser, and the parser only ever sees a
 * [com.intellij.lang.PsiBuilder]. This re-implements the stock
 * [IFileElementType.doParseContents] (the platform's own three lines) purely so
 * the builder can be tagged before the parser runs. Everything downstream then
 * asks the builder, not the file.
 */
private object JuxFileElementType : IFileElementType("JUX_FILE", JuxLanguage) {
    override fun doParseContents(chameleon: ASTNode, psi: PsiElement): ASTNode? {
        val project = psi.project
        val language = getLanguageForParser(psi)
        val builder = PsiBuilderFactory.getInstance()
            .createBuilder(project, chameleon, null, language, chameleon.chars)
        // `psi` IS the file for a file element type; `containingFile` keeps it
        // right even if the platform ever hands us an inner element.
        val name = (psi as? PsiFile ?: psi.containingFile)?.name
        if (isJuxStubFileName(name)) builder.markForeignStub()
        val parser = LanguageParserDefinitions.INSTANCE.forLanguage(language).createParser(project)
        return parser.parse(this, builder).firstChildNode
    }
}

/** The root node type for a parsed `.jux` (or `.jux.d`) file. */
val JUX_FILE: IFileElementType = JuxFileElementType

/**
 * The PSI root for a `.jux` source file. Backed by [JuxParserDefinition], which
 * resolves the lexer + parser that build the tree under this node.
 */
class JuxFile(viewProvider: FileViewProvider) : PsiFileBase(viewProvider, JuxLanguage) {
    override fun getFileType(): FileType = JuxFileType
    override fun toString(): String = "Jux File"
}
