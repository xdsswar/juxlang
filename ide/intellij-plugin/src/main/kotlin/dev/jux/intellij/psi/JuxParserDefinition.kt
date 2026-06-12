package dev.jux.intellij.psi

import com.intellij.lang.ASTNode
import com.intellij.lang.ParserDefinition
import com.intellij.lang.PsiParser
import com.intellij.lexer.Lexer
import com.intellij.openapi.project.Project
import com.intellij.psi.FileViewProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.tree.IFileElementType
import com.intellij.psi.tree.TokenSet
import dev.jux.intellij.highlight.JuxLexer
import dev.jux.intellij.highlight.JuxTokenTypes
import dev.jux.intellij.parser.JuxParser
import dev.jux.intellij.psi.JuxElementTypes as E

/**
 * Wires the Jux language into the platform's PSI machinery: the lexer, the
 * parser, the file node type, and the element-type → PSI-impl mapping. Modelled
 * on `JavaParserDefinition`. Registered via `lang.parserDefinition` in
 * `plugin.xml`.
 */
class JuxParserDefinition : ParserDefinition {
    override fun createLexer(project: Project?): Lexer = JuxLexer()

    override fun createParser(project: Project?): PsiParser = JuxParser()

    override fun getFileNodeType(): IFileElementType = JUX_FILE

    override fun getCommentTokens(): TokenSet = JuxTokenTypes.COMMENTS

    override fun getStringLiteralElements(): TokenSet = JuxTokenTypes.STRING_LITERALS

    override fun createFile(viewProvider: FileViewProvider): PsiFile = JuxFile(viewProvider)

    /**
     * The element-type → PSI-impl switch. The contract requires the PSI type to
     * be determined solely by the node's element type (never its content).
     * Named declarations get typed elements (so rename/Structure View see their
     * identifier); everything else uses the generic composite element.
     */
    override fun createElement(node: ASTNode): PsiElement = when (node.elementType) {
        E.CLASS_DECLARATION,
        E.INTERFACE_DECLARATION,
        E.ENUM_DECLARATION,
        E.RECORD_DECLARATION,
        E.STRUCT_DECLARATION,
        E.ANNOTATION_DECLARATION,
        E.TYPE_ALIAS_DECLARATION -> JuxTypeDeclaration(node)

        E.METHOD_DECLARATION,
        E.CONSTRUCTOR_DECLARATION,
        E.OPERATOR_DECLARATION -> JuxMethodDeclaration(node)

        E.FIELD_DECLARATION,
        E.CONST_DECLARATION -> JuxFieldDeclaration(node)

        // Properties get their own PSI class (a JuxFieldDeclaration subclass,
        // so field consumers keep working) with §P accessor helpers.
        E.PROPERTY_DECLARATION -> JuxPropertyDeclaration(node)

        E.ENUM_CONSTANT -> JuxEnumConstant(node)
        E.PARAMETER -> JuxParameter(node)
        E.LOCAL_VARIABLE -> JuxLocalVariable(node)

        else -> JuxCompositeElement(node)
    }
}
