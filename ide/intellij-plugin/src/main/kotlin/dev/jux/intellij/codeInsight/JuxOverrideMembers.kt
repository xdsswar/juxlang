package dev.jux.intellij.codeInsight

import com.intellij.codeInsight.generation.ClassMember
import com.intellij.codeInsight.generation.MemberChooserObject
import com.intellij.codeInsight.generation.MemberChooserObjectBase
import com.intellij.codeInsight.generation.PsiElementMemberChooserObject
import com.intellij.icons.AllIcons
import com.intellij.ide.util.MemberChooser
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.editor.Editor
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.psi.JuxElementTypes
import dev.jux.intellij.psi.JuxMethodDeclaration
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * The shared implement/override engine behind Ctrl+I / Ctrl+O
 * ([JuxImplementMembersHandler] / [JuxOverrideMembersHandler]), the Alt+Insert
 * Generate action, and the E0429 missing-implementation quick-fix.
 *
 * Candidate collection walks the supertype chain breadth-first (extends before
 * implements, nearest declaration wins) with cross-file resolution through
 * [JuxTypeIndex] and a cycle guard. Methods match by **name + arity** — the
 * same approximation [JuxHierarchy] documents. Signatures are textual
 * ([JuxHierarchy.methodSignature]); generic substitution
 * (`extends Box<int>` stubbing `T get()` as `int get()`) is a known
 * follow-up, matching the pre-engine generator's behavior.
 */
object JuxOverrideMembers {

    /** What inserting the stub means for the subclass. */
    enum class Kind {
        /** The super-method is abstract (body-less) — the class MUST provide it. */
        IMPLEMENT,

        /** The super-method has a body — the stub overrides and delegates to `super`. */
        OVERRIDE,
    }

    /** One inheritable method the class at hand does not declare yet. */
    data class Candidate(
        val method: JuxMethodDeclaration,
        val ownerName: String,
        val signature: String,
        val kind: Kind,
    )

    /**
     * Every supertype method [type] can implement or override and has not
     * declared itself, nearest-declaration-first. A method abstract in an
     * interface but implemented by a nearer concrete ancestor classifies as
     * OVERRIDE (the inherited body already satisfies the contract).
     */
    fun candidates(type: JuxTypeDeclaration): List<Candidate> {
        val project = type.project
        val out = ArrayList<Candidate>()
        // name+arity keys — own declarations exclude, and the FIRST (nearest)
        // super declaration wins over farther redeclarations.
        val seen = HashSet<String>()
        for (m in JuxHierarchy.directChildren(type, JuxElementTypes.METHOD_DECLARATION)) {
            val name = (m as? JuxNamedElement)?.name ?: continue
            seen.add("$name/${JuxHierarchy.arity(m)}")
        }

        val queue = ArrayDeque(JuxHierarchy.superTypeNames(type))
        val visitedTypes = HashSet<String>()
        while (queue.isNotEmpty()) {
            val superName = queue.removeFirst()
            if (!visitedTypes.add(superName)) continue
            val superDecl = JuxTypeIndex.findType(project, superName) ?: continue
            for (m in JuxHierarchy.directChildren(superDecl, JuxElementTypes.METHOD_DECLARATION)) {
                val method = m as? JuxMethodDeclaration ?: continue
                val name = method.name ?: continue
                if (!seen.add("$name/${JuxHierarchy.arity(method)}")) continue
                if (!JuxHierarchy.isOverridable(method)) continue
                val sig = JuxHierarchy.methodSignature(method) ?: continue
                val kind = if (JuxHierarchy.hasBody(method)) Kind.OVERRIDE else Kind.IMPLEMENT
                out.add(Candidate(method, superName, sig, kind))
            }
            queue.addAll(JuxHierarchy.superTypeNames(superDecl))
        }
        return out
    }

    /**
     * The stub text for one candidate, [indent] being the member indentation
     * (one level inside the class body):
     *
     *  - IMPLEMENT: a `throw new UnsupportedOperationException` body — valid
     *    for any return type, so the result always compiles;
     *  - OVERRIDE: delegate to `super.name(args)`, `return`ing unless void.
     */
    fun stubText(c: Candidate, indent: String): String {
        val body = when (c.kind) {
            Kind.IMPLEMENT ->
                "throw new UnsupportedOperationException(\"TODO: ${c.method.name}\");"
            Kind.OVERRIDE -> {
                val call = "super.${c.method.name}(${JuxHierarchy.parameterNames(c.method).joinToString(", ")});"
                if (JuxHierarchy.returnTypeText(c.method) == "void") call else "return $call"
            }
        }
        return "\n$indent@override\n$indent" + "public ${c.signature} {\n$indent    $body\n$indent}\n"
    }

    /**
     * Inserts the stubs at the end of [type]'s body (before its closing `}`),
     * in one undoable write command — correct wherever the caret sits when a
     * handler is invoked.
     */
    fun insertStubs(project: Project, type: JuxTypeDeclaration, chosen: List<Candidate>) {
        if (chosen.isEmpty()) return
        val body = type.node.findChildByType(JuxElementTypes.CLASS_BODY)?.psi ?: return
        val file = type.containingFile ?: return
        val docMgr = PsiDocumentManager.getInstance(project)
        val doc = docMgr.getDocument(file) ?: return

        // Insert just before the body's closing brace; member indent = the
        // class's own line indent plus one level.
        val closeOffset = body.textRange.endOffset - 1
        val classIndent = indentAt(doc, type.textRange.startOffset)
        val memberIndent = "$classIndent    "
        val text = chosen.joinToString("") { stubText(it, memberIndent) } + classIndent

        val write = {
            doc.insertString(closeOffset, text)
            docMgr.commitDocument(doc)
        }
        // Quick-fixes already run inside a write command; the handlers/actions
        // don't and need their own.
        if (ApplicationManager.getApplication().isWriteAccessAllowed) {
            write()
        } else {
            WriteCommandAction.runWriteCommandAction(project, "Implement/Override Methods", null, write)
        }
    }

    /**
     * Member-chooser front door for the handlers and the Generate action:
     * filter to [kinds], pop the platform [MemberChooser] (select-all in unit
     * tests / headless), insert what the user picked. Returns false when there
     * was nothing to offer (callers show their own hint).
     */
    fun chooseAndInsert(
        project: Project,
        editor: Editor?,
        type: JuxTypeDeclaration,
        kinds: Set<Kind>,
        title: String,
    ): Boolean {
        // The walk resolves supertypes project-wide — on a large project that
        // is real work, and the handlers invoke on the EDT. Collect under a
        // cancellable progress (read action on a pooled thread) so the UI
        // never freezes; unit tests / headless run it inline.
        val app = ApplicationManager.getApplication()
        val all: List<Candidate> =
            if (app.isUnitTestMode || app.isHeadlessEnvironment) {
                candidates(type).filter { it.kind in kinds }
            } else {
                com.intellij.openapi.progress.ProgressManager.getInstance()
                    .runProcessWithProgressSynchronously<List<Candidate>, RuntimeException>(
                        { app.runReadAction<List<Candidate>> { candidates(type).filter { it.kind in kinds } } },
                        "Collecting inherited methods...",
                        true,
                        project,
                    )
            }
        if (all.isEmpty()) return false

        val chosen: List<Candidate> =
            if (ApplicationManager.getApplication().isUnitTestMode || ApplicationManager.getApplication().isHeadlessEnvironment) {
                all
            } else {
                val items = all.map { JuxMemberChooserObject(it) }.toTypedArray()
                val chooser = MemberChooser(items, false, true, project)
                chooser.title = title
                chooser.selectElements(items)
                if (!chooser.showAndGet()) return true // cancelled — handled, insert nothing
                chooser.selectedElements?.map { it.candidate } ?: emptyList()
            }
        insertStubs(project, type, chosen)
        return true
    }

    /** The Jux type declaration enclosing the caret, or null. */
    fun typeAtCaret(editor: Editor, file: PsiFile): JuxTypeDeclaration? {
        if (file.language != JuxLanguage) return null
        val at = file.findElementAt(editor.caretModel.offset) ?: return null
        return PsiTreeUtil.getParentOfType(at, JuxTypeDeclaration::class.java)
    }

    /** The whitespace prefix of the line containing [offset]. */
    private fun indentAt(doc: com.intellij.openapi.editor.Document, offset: Int): String {
        val lineStart = doc.getLineStartOffset(doc.getLineNumber(offset))
        return doc.charsSequence.subSequence(lineStart, offset).takeWhile { it == ' ' || it == '\t' }.toString()
    }

    /** A chooser row: `ReturnType name(params)` grouped under its owner type. */
    private class JuxMemberChooserObject(val candidate: Candidate) :
        PsiElementMemberChooserObject(candidate.method as PsiElement, candidate.signature, AllIcons.Nodes.Method),
        ClassMember {
        override fun getParentNodeDelegate(): MemberChooserObject =
            MemberChooserObjectBase(candidate.ownerName, AllIcons.Nodes.Class)
    }
}
