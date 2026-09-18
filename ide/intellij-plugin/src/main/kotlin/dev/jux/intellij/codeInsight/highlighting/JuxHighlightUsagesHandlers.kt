package dev.jux.intellij.codeInsight.highlighting

import com.intellij.codeInsight.highlighting.HighlightUsagesHandlerBase
import com.intellij.codeInsight.highlighting.HighlightUsagesHandlerFactoryBase
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.DumbAware
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.elementType
import com.intellij.util.Consumer
import dev.jux.intellij.codeInsight.JuxGotoSuperHandler
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * Highlight Usages (Ctrl+Shift+F7, or automatically with the caret) on a
 * keyword, the way Java's handlers work:
 *
 * - `return` / `throw`: every exit point of the enclosing method or lambda
 *   ([JuxExitPointsHandler]).
 * - `break` / `continue`: the loop or switch it leaves and every other
 *   `break` / `continue` leaving it ([JuxBreakOutsHandler]).
 * - `try`: what its body throws and does not catch inside; `catch`: what that
 *   clause catches; `throws`: what the method body throws
 *   ([JuxExceptionSitesHandler]).
 * - `extends` / `implements`: the methods that override or implement a member
 *   of the named types ([JuxOverridingMethodsHandler]).
 */
class JuxHighlightUsagesHandlerFactory : HighlightUsagesHandlerFactoryBase(), DumbAware {

    override fun createHighlightUsagesHandler(
        editor: Editor,
        file: PsiFile,
        target: PsiElement,
    ): HighlightUsagesHandlerBase<*>? {
        if (file !is JuxFile) return null
        val parent = target.parent ?: return null
        return when (target.elementType) {
            T.RETURN_KW, T.THROW_KW -> JuxExitPointsHandler(editor, file, target)
            T.BREAK_KW, T.CONTINUE_KW -> JuxBreakOutsHandler(editor, file, target)
            T.TRY_KW -> {
                if (parent.elementType !== E.TRY_STATEMENT) return null
                val body = parent.node.findChildByType(E.CODE_BLOCK)?.psi ?: return null
                JuxExceptionSitesHandler(editor, file, target, JuxExceptionSites.unhandled(body))
            }
            T.CATCH_KW -> {
                if (parent.elementType !== E.CATCH_CLAUSE) return null
                JuxExceptionSitesHandler(editor, file, target, JuxExceptionSites.caughtBy(parent))
            }
            T.THROWS_KW -> {
                val method = parent.parent as? JuxMethodDeclaration ?: return null
                val body = method.node.findChildByType(E.CODE_BLOCK)?.psi ?: return null
                JuxExceptionSitesHandler(editor, file, target, JuxExceptionSites.unhandled(body))
            }
            T.EXTENDS_KW, T.IMPLEMENTS_KW -> {
                val type = parent.parent as? JuxTypeDeclaration ?: return null
                JuxOverridingMethodsHandler(editor, file, target, type, parent)
            }
            else -> null
        }
    }
}

/** Shared shape: the keyword under the caret is the one target. */
abstract class JuxKeywordHandler(editor: Editor, file: PsiFile, protected val keyword: PsiElement) :
    HighlightUsagesHandlerBase<PsiElement>(editor, file) {

    override fun getTargets(): List<PsiElement> = listOf(keyword)

    override fun selectTargets(targets: List<PsiElement>, selectionConsumer: Consumer<in List<PsiElement>>) {
        selectionConsumer.consume(targets)
    }
}

/**
 * Every `return` and `throw` that leaves the enclosing method or lambda. A
 * `throw` caught by an enclosing `try` in the same body is not an exit; a
 * nested lambda's statements belong to the lambda.
 */
class JuxExitPointsHandler(editor: Editor, file: PsiFile, keyword: PsiElement) :
    JuxKeywordHandler(editor, file, keyword) {

    override fun computeUsages(targets: List<PsiElement>) {
        val owner = JuxExceptionSites.enclosingBody(keyword) ?: return
        val body = owner.node.findChildByType(E.CODE_BLOCK)?.psi ?: return
        for (statement in JuxExceptionSites.statementsOf(body, setOf(E.RETURN_STATEMENT))) {
            addOccurrence(statement)
        }
        for (site in JuxExceptionSites.unhandled(body)) {
            if (site.element.elementType === E.THROW_STATEMENT) addOccurrence(site.element)
        }
    }
}

/**
 * The loop (or switch) a `break` / `continue` leaves, and the other `break` /
 * `continue` statements leaving the same one. Labels are followed.
 */
class JuxBreakOutsHandler(editor: Editor, file: PsiFile, keyword: PsiElement) :
    JuxKeywordHandler(editor, file, keyword) {

    override fun computeUsages(targets: List<PsiElement>) {
        val statement = keyword.parent ?: return
        val exited = exitedStatement(statement)
        if (exited != null) {
            exited.firstChild?.let { addOccurrence(it) }
            // A do-while is named by its `while` too.
            if (exited.elementType === E.DO_WHILE_STATEMENT) {
                exited.node.findChildByType(T.WHILE_KW)?.psi?.let { addOccurrence(it) }
            }
            for (other in JuxExceptionSites.statementsOf(exited, setOf(E.BREAK_STATEMENT, E.CONTINUE_STATEMENT))) {
                if (other != statement && exitedStatement(other) == exited) other.firstChild?.let { addOccurrence(it) }
            }
        }
        addOccurrence(keyword)
    }

    companion object {
        private val LOOPS = setOf(E.WHILE_STATEMENT, E.DO_WHILE_STATEMENT, E.FOR_STATEMENT, E.FOR_EACH_STATEMENT)

        /**
         * The statement [jump] (a `break` or `continue`) leaves: the labeled
         * one when it names a label, else the nearest loop, or for `break`
         * the nearest switch as well. Never crosses a method or lambda.
         */
        fun exitedStatement(jump: PsiElement): PsiElement? {
            val isBreak = jump.elementType === E.BREAK_STATEMENT
            val label = jump.node.findChildByType(T.IDENTIFIER)?.text
            var p: PsiElement? = jump.parent
            while (p != null && p !is JuxFile) {
                val t = p.elementType
                if (t === E.LAMBDA_EXPRESSION || p is JuxMethodDeclaration) return null
                if (label != null) {
                    if (t === E.LABELED_STATEMENT && p.firstChild?.text == label) {
                        return p.children.firstOrNull { it.elementType in LOOPS || it.elementType === E.SWITCH_STATEMENT }
                    }
                } else if (t in LOOPS || (isBreak && t === E.SWITCH_STATEMENT)) {
                    return p
                }
                p = p.parent
            }
            return null
        }
    }
}

/** Highlights a precomputed set of places that throw. */
class JuxExceptionSitesHandler(
    editor: Editor,
    file: PsiFile,
    keyword: PsiElement,
    private val sites: List<JuxExceptionSite>,
) : JuxKeywordHandler(editor, file, keyword) {

    override fun computeUsages(targets: List<PsiElement>) {
        for (site in sites) addOccurrence(site.element)
        addOccurrence(keyword)
    }
}

/**
 * On `extends` / `implements`: the methods of the type that override or
 * implement a member of the types that clause names.
 */
class JuxOverridingMethodsHandler(
    editor: Editor,
    file: PsiFile,
    keyword: PsiElement,
    private val type: JuxTypeDeclaration,
    private val clause: PsiElement,
) : JuxKeywordHandler(editor, file, keyword) {

    override fun computeUsages(targets: List<PsiElement>) {
        val named = clause.children
            .filter { it.elementType === E.TYPE_REFERENCE }
            .map { JuxHierarchy.bareTypeName(it) }
            .toSet()
        for (m in JuxHierarchy.directChildren(type, E.METHOD_DECLARATION)) {
            val method = m as? JuxMethodDeclaration ?: continue
            val fromClause = JuxGotoSuperHandler.superMethods(method).any { sup ->
                val owner = JuxHierarchy.enclosingType(sup) ?: return@any false
                owner.name in named || named.any { JuxHierarchy.inheritsFrom(owner, it) } ||
                    named.any { n -> JuxTypeIndex.findType(type, n)?.let { JuxHierarchy.inheritsFrom(it, owner.name) } == true }
            }
            if (fromClause) method.nameIdentifier?.let { addOccurrence(it) }
        }
        addOccurrence(keyword)
    }
}

/** A place that throws: the statement or call, and the exception type's name when known. */
data class JuxExceptionSite(val element: PsiElement, val type: String?)

/**
 * What a block throws, found in the PSI: `throw` statements and calls to
 * methods whose `throws` clause resolves in the project. A site is left out
 * when a `try` inside the block catches it, and a nested lambda's body is its
 * own. Types match by name and by the project's declared hierarchy;
 * `Throwable` catches everything, and `Exception` everything but an `...Error`.
 */
object JuxExceptionSites {

    /** Every site in [block] nothing inside [block] catches. */
    fun unhandled(block: PsiElement): List<JuxExceptionSite> {
        val out = ArrayList<JuxExceptionSite>()
        collect(block, out)
        return out
    }

    /**
     * The sites of the try body that [catchClause] catches: those assignable
     * to its type and not already caught by an earlier clause.
     */
    fun caughtBy(catchClause: PsiElement): List<JuxExceptionSite> {
        val tryStatement = catchClause.parent ?: return emptyList()
        val body = tryStatement.node.findChildByType(E.CODE_BLOCK)?.psi ?: return emptyList()
        val clauses = tryStatement.children.filter { it.elementType === E.CATCH_CLAUSE }
        val index = clauses.indexOf(catchClause)
        return unhandled(body).filter { site ->
            val first = clauses.indexOfFirst { clause -> catchTypes(clause).any { catches(it, site.type, clause) } }
            first == index
        }
    }

    private fun collect(element: PsiElement, out: MutableList<JuxExceptionSite>) {
        for (child in element.children) {
            when (child.elementType) {
                // A lambda throws when it runs, not where it is written.
                E.LAMBDA_EXPRESSION -> continue
                E.THROW_STATEMENT -> out.add(JuxExceptionSite(child, thrownType(child)))
                E.CALL_EXPRESSION -> {
                    for (type in declaredThrows(child)) out.add(JuxExceptionSite(child, type))
                }
                E.TRY_STATEMENT -> {
                    // What the inner try's body throws and none of its catch
                    // clauses catch still leaves it; its catch and finally
                    // blocks throw outward as written.
                    val body = child.node.findChildByType(E.CODE_BLOCK)?.psi
                    val clauses = child.children.filter { it.elementType === E.CATCH_CLAUSE }
                    if (body != null) {
                        val inner = ArrayList<JuxExceptionSite>()
                        collect(body, inner)
                        out.addAll(inner.filter { site ->
                            clauses.none { clause -> catchTypes(clause).any { catches(it, site.type, clause) } }
                        })
                    }
                    for (other in child.children) if (other != body) collect(other, out)
                    continue
                }
            }
            collect(child, out)
        }
    }

    /** The type names a catch clause names (`catch (A | B e)` gives both). */
    private fun catchTypes(clause: PsiElement): List<String> {
        val binding = clause.node.findChildByType(E.LOCAL_VARIABLE)?.psi ?: return emptyList()
        return binding.children
            .filter { it.elementType === E.TYPE_REFERENCE }
            .map { JuxHierarchy.bareTypeName(it) }
            .ifEmpty { listOf("Throwable") }
    }

    /** `throw new X(...)` gives `X`; `throw e` the declared type of `e`. */
    private fun thrownType(statement: PsiElement): String? {
        val value = statement.children.firstOrNull() ?: return null
        if (value.elementType === E.NEW_EXPRESSION) {
            return value.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let { JuxHierarchy.bareTypeName(it) }
        }
        val declaration = value.references.firstNotNullOfOrNull { it.resolve() } ?: return null
        return declaration.node.findChildByType(E.TYPE_REFERENCE)?.psi?.let { JuxHierarchy.bareTypeName(it) }
    }

    /** The exception types a call's target declares in its `throws` clause. */
    private fun declaredThrows(call: PsiElement): List<String> {
        val target = call.firstChild?.references?.firstNotNullOfOrNull { it.resolve() } as? JuxMethodDeclaration
            ?: return emptyList()
        val clause = target.node.findChildByType(E.THROWS_CLAUSE)?.psi ?: return emptyList()
        return clause.children.filter { it.elementType === E.TYPE_REFERENCE }.map { JuxHierarchy.bareTypeName(it) }
    }

    /** Whether a clause of type [catchType] catches an exception of type [thrown]. */
    fun catches(catchType: String, thrown: String?, context: PsiElement): Boolean {
        if (catchType == "Throwable") return true
        if (thrown == null) return catchType == "Exception"
        if (thrown == catchType) return true
        if (catchType == "Exception") return !thrown.endsWith("Error")
        val thrownDecl = JuxTypeIndex.findType(context, thrown) ?: return false
        return JuxHierarchy.inheritsFrom(thrownDecl, catchType)
    }

    /** The method, constructor, operator or lambda [element] is inside. */
    fun enclosingBody(element: PsiElement): PsiElement? {
        var p: PsiElement? = element.parent
        while (p != null && p !is JuxFile) {
            if (p.elementType === E.LAMBDA_EXPRESSION || p is JuxMethodDeclaration) return p
            p = p.parent
        }
        return null
    }

    /** The statements of [kinds] under [root], not inside a nested lambda or method. */
    fun statementsOf(root: PsiElement, kinds: Set<com.intellij.psi.tree.IElementType>): List<PsiElement> {
        val out = ArrayList<PsiElement>()
        fun walk(e: PsiElement) {
            for (child in e.children) {
                if (child.elementType === E.LAMBDA_EXPRESSION || child is JuxMethodDeclaration) continue
                if (child.elementType in kinds) out.add(child)
                walk(child)
            }
        }
        walk(root)
        return out
    }
}
