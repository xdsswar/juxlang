package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.LocalQuickFixAndIntentionActionOnPsiElement
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementFactory
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.quickfix.JuxCreateVariables
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * "Unhandled exception", Java's compile error for a checked exception that is
 * neither caught nor declared (JUX-EXCEPTIONS-ADDENDUM §X.1.3, §X.3).
 *
 * The IDE decides checkedness by the spec's own rule (§X.1.4): an exception
 * class is checked when its `extends` chain reaches `Exception` without
 * passing `RuntimeException`. The chain is followed through the project's
 * declarations; one that leaves them anywhere else (a library class, the
 * built-in subclasses) is not judged, so only project-declared exceptions are
 * ever reported. Two throw points are seen:
 *
 * - `throw new E(...)`;
 * - a call to a method the IDE resolves whose `throws` clause names `E`.
 *
 * Inside a lambda nothing is reported (its function type may declare what it
 * throws), nor anywhere outside a method or constructor body. The fixes are
 * Java's: add the exception to the method's `throws`, or wrap the statement
 * in `try` / `catch`.
 */
class JuxUnhandledExceptionInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                val thrown: List<Pair<String, List<String>>> = when (element.elementType) {
                    E.THROW_STATEMENT -> thrownByThrow(element)
                    E.CALL_EXPRESSION -> thrownByCall(element)
                    else -> return
                }
                if (thrown.isEmpty()) return
                val method = enclosingCallable(element) ?: return
                for ((name, chain) in thrown) {
                    if (isHandled(element, method, chain)) continue
                    val anchor = if (element.elementType === E.CALL_EXPRESSION) element.firstChild ?: element else element
                    val fixes = ArrayList<LocalQuickFix>()
                    fixes.add(AddToThrowsFix(method, name))
                    val stmt = JuxCreateVariables.statementAnchor(element)
                    if (stmt != null && stmt.elementType !== E.LOCAL_VARIABLE) fixes.add(SurroundWithTryCatchFix(stmt, name))
                    holder.registerProblem(anchor, "Unhandled exception: $name", ProblemHighlightType.GENERIC_ERROR, *fixes.toTypedArray())
                }
            }
        }

    /** `throw new E(...)`: E and its chain, when E is a checked project exception. */
    private fun thrownByThrow(stmt: PsiElement): List<Pair<String, List<String>>> {
        val newExpr = JuxTypeEngine.firstExpressionChild(stmt)?.takeIf { it.elementType === E.NEW_EXPRESSION } ?: return emptyList()
        val ref = newExpr.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return emptyList()
        return listOfNotNull(checkedChain(ref))
    }

    /** A call to a resolvable method: each checked project exception in its `throws`. */
    private fun thrownByCall(call: PsiElement): List<Pair<String, List<String>>> {
        val target = dev.jux.intellij.quickfix.JuxCreateFromUsage.resolveCallee(call) ?: return emptyList()
        val clause = target.node.findChildByType(E.THROWS_CLAUSE)?.psi ?: return emptyList()
        return clause.children.filter { it.elementType === E.TYPE_REFERENCE }.mapNotNull { checkedChain(it) }
    }

    /**
     * For a type reference naming a project exception class: its bare name
     * and the names along its `extends` chain (itself first, `Exception`
     * last), when that chain makes it checked. Null for anything unchecked
     * or undecidable.
     */
    private fun checkedChain(ref: PsiElement): Pair<String, List<String>>? {
        val start = (JuxTypeEngine.typeOfTypeReference(ref) as? JuxType.ClassType)?.decl ?: return null
        val names = ArrayList<String>()
        var decl: JuxTypeDeclaration = start
        repeat(32) {
            val name = decl.name ?: return null
            if (name == "RuntimeException") return null
            names.add(name)
            if (name == "Exception") return (start.name ?: return null) to names
            val parentRef = JuxHierarchy.supertypeReferences(decl).firstOrNull { it.second }?.first ?: return null
            val parentName = JuxHierarchy.bareTypeName(parentRef)
            val parent = JuxTypeIndex.findType(decl, parentName)
            if (parent == null) {
                // The chain leaves the project. Only the two names the spec
                // defines checkedness by can still decide it.
                return when (parentName) {
                    "Exception" -> { names.add(parentName); (start.name ?: return null) to names }
                    else -> null
                }
            }
            decl = parent
        }
        return null
    }

    /** The method or constructor body [e] runs in; null in a lambda, an initializer, or at top level. */
    private fun enclosingCallable(e: PsiElement): PsiElement? {
        var p: PsiElement? = e.parent
        while (p != null && p !is JuxFile) {
            when (p.elementType) {
                E.LAMBDA_EXPRESSION, E.CLASS_BODY, E.PROPERTY_ACCESSOR -> return null
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION -> return p
            }
            p = p.parent
        }
        return null
    }

    /** Caught by an enclosing `try`, or declared in the method's `throws`. */
    private fun isHandled(site: PsiElement, method: PsiElement, chain: List<String>): Boolean {
        var p: PsiElement? = site
        while (p != null && p !== method) {
            val parent = p.parent ?: break
            // Only the guarded block of a `try`, not its catch or finally parts.
            if (parent.elementType === E.TRY_STATEMENT && p.elementType === E.CODE_BLOCK &&
                parent.node.findChildByType(E.CODE_BLOCK)?.psi === p
            ) {
                for (clause in parent.children.filter { it.elementType === E.CATCH_CLAUSE }) {
                    val caught = PsiTreeUtil.collectElements(clause) { it.elementType === E.TYPE_REFERENCE }
                        .map { JuxHierarchy.bareTypeName(it) }
                    if (caught.any { it in chain || it == "Throwable" }) return true
                }
            }
            p = parent
        }
        val declared = method.node.findChildByType(E.THROWS_CLAUSE)?.psi?.children
            ?.filter { it.elementType === E.TYPE_REFERENCE }
            ?.map { JuxHierarchy.bareTypeName(it) }.orEmpty()
        return declared.any { it in chain || it == "Throwable" }
    }

    /** Adds `E` to the method's `throws` clause, creating the clause if needed. */
    private class AddToThrowsFix(method: PsiElement, private val exception: String) :
        LocalQuickFixAndIntentionActionOnPsiElement(method) {
        override fun getText(): String = "Add exception to method signature"

        override fun getFamilyName(): String = "Add exception to method signature"

        override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
            val existing = startElement.node.findChildByType(E.THROWS_CLAUSE)?.psi
            val names = existing?.children?.filter { it.elementType === E.TYPE_REFERENCE }?.map { it.text.trim() }.orEmpty()
            val clause = throwsClause(project, (names + exception).joinToString(", "))
            if (existing != null) {
                existing.replace(clause)
                return
            }
            val params = startElement.node.findChildByType(E.PARAMETER_LIST)?.psi ?: return
            val at = params.textRange.endOffset
            JuxCodeFacts.edit(project, file, at, at, " ${clause.text}")
        }

        private fun throwsClause(project: Project, list: String): PsiElement {
            val file = JuxElementFactory.createFile(project, "void __jux() throws $list {}\n")
            return PsiTreeUtil.collectElements(file) { it.elementType === E.THROWS_CLAUSE }.first()
        }
    }

    /** Wraps the statement in `try { … } catch (E e) { … }`, the catch body left to the user. */
    private class SurroundWithTryCatchFix(statement: PsiElement, private val exception: String) :
        LocalQuickFixAndIntentionActionOnPsiElement(statement) {
        override fun getText(): String = "Surround with try/catch"

        override fun getFamilyName(): String = "Surround with try/catch"

        override fun invoke(project: Project, file: PsiFile, editor: Editor?, startElement: PsiElement, endElement: PsiElement) {
            val text = "try {\n${startElement.text}\n} catch ($exception e) {\n    // handle the $exception\n}"
            val wrapped = startElement.replace(JuxElementFactory.createStatement(project, text))
            JuxCodeFacts.reformat(project, wrapped)
        }
    }
}
