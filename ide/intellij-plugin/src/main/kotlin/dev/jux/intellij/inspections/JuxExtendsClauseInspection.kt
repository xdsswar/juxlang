package dev.jux.intellij.inspections

import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy
import dev.jux.intellij.resolve.JuxTypeIndex

/**
 * The Jux inheritance shape, enforced on the `extends` clause IDE-side with
 * the compiler's wording:
 *
 *  - a class extends exactly ONE other **class** — a second entry is a
 *    single-inheritance error (structural, no resolution needed);
 *  - extending an interface/record/enum/… is E0423 (quick-fix: move an
 *    interface to `implements`);
 *  - extending a `final` class is E0420; a `sealed` class whose `permits`
 *    clause doesn't list this type is E0422;
 *  - an interface may extend only interfaces.
 *
 * Resolution is project-wide via [JuxTypeIndex]; **unresolved names stay
 * silent** (std / library supertypes must never false-error).
 */
class JuxExtendsClauseInspection : LocalInspectionTool() {

    override fun checkFile(file: PsiFile, manager: InspectionManager, isOnTheFly: Boolean): Array<ProblemDescriptor>? {
        if (file !is JuxFile) return null
        val problems = ArrayList<ProblemDescriptor>()

        fun report(target: PsiElement, message: String, fix: LocalQuickFix? = null) {
            problems.add(
                manager.createProblemDescriptor(target, message, fix, ProblemHighlightType.ERROR, isOnTheFly),
            )
        }

        for (type in PsiTreeUtil.findChildrenOfType(file, JuxTypeDeclaration::class.java)) {
            val refs = JuxHierarchy.supertypeReferences(type).filter { it.second }.map { it.first }
            if (refs.isEmpty()) continue
            val typeName = type.name ?: continue

            if (JuxHierarchy.isInterface(type)) {
                // Interfaces extend any number of interfaces — only the kind
                // of each (resolved) entry needs checking.
                for (ref in refs) {
                    val parent = JuxTypeIndex.findType(type.project, JuxHierarchy.bareTypeName(ref)) ?: continue
                    if (!JuxHierarchy.isInterface(parent)) {
                        report(
                            ref,
                            "Interface '$typeName' cannot extend '${parent.name}' because it is " +
                                "${JuxHierarchy.kindWord(parent)} (interfaces extend only interfaces)",
                        )
                    }
                }
                continue
            }
            if (!JuxHierarchy.isClass(type)) continue // records/enums: implements-only kinds

            // Single inheritance is structural — entries past the first are
            // wrong no matter what they resolve to.
            for (extra in refs.drop(1)) {
                val resolved = JuxTypeIndex.findType(type.project, JuxHierarchy.bareTypeName(extra))
                val fix = if (resolved != null && JuxHierarchy.isInterface(resolved)) MoveToImplementsFix() else null
                report(
                    extra,
                    "A class can extend at most one class (single inheritance); " +
                        "interfaces belong in the 'implements' clause",
                    fix,
                )
            }

            val first = refs.first()
            val parent = JuxTypeIndex.findType(type.project, JuxHierarchy.bareTypeName(first)) ?: continue
            when {
                !JuxHierarchy.isClass(parent) -> report(
                    first,
                    "Class '$typeName' cannot extend '${parent.name}' because it is " +
                        "${JuxHierarchy.kindWord(parent)} (only classes are extensible) (E0423)",
                    if (JuxHierarchy.isInterface(parent)) MoveToImplementsFix() else null,
                )
                JuxHierarchy.hasModifier(parent, "final") -> report(
                    first,
                    "Class '$typeName' cannot extend '${parent.name}' because '${parent.name}' " +
                        "is declared final (E0420)",
                )
                JuxHierarchy.hasModifier(parent, "sealed") && !permits(parent, typeName) -> report(
                    first,
                    "Class '$typeName' is not permitted to extend '${parent.name}' " +
                        "(not listed in its permits clause) (E0422)",
                )
            }
        }
        return problems.toTypedArray()
    }

    /** Whether the sealed [parent]'s `permits` clause names [childName]. */
    private fun permits(parent: JuxTypeDeclaration, childName: String): Boolean {
        val clause = parent.node.findChildByType(E.PERMITS_CLAUSE)?.psi
            ?: return true // sealed without permits: leave the rule to the compiler
        return PsiTreeUtil.findChildrenOfType(clause, PsiElement::class.java)
            .any { it.node.elementType == E.TYPE_REFERENCE && JuxHierarchy.bareTypeName(it) == childName }
    }
}

/**
 * Moves a TYPE_REFERENCE out of the `extends` clause into the `implements`
 * clause (creating one before the body/permits when absent). Pure document
 * surgery, removal applied after insertion-offset computation so offsets stay
 * valid (the insertion point always lies AFTER the removed range).
 */
internal class MoveToImplementsFix : LocalQuickFix {
    override fun getFamilyName(): String = "Change to implements"

    override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
        val ref = descriptor.psiElement ?: return
        val clause = ref.parent?.takeIf { it.node.elementType == E.EXTENDS_CLAUSE } ?: return
        val type = PsiTreeUtil.getParentOfType(ref, JuxTypeDeclaration::class.java) ?: return
        JuxSupertypeClauseSurgery.move(
            project, type, ref, fromClause = clause,
            toClauseType = E.IMPLEMENTS_CLAUSE, toKeyword = "implements",
        )
    }
}

/** Inverse of [MoveToImplementsFix]: `implements X` → `extends X` (slot free). */
internal class MoveToExtendsFix : LocalQuickFix {
    override fun getFamilyName(): String = "Move to extends"

    override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
        val ref = descriptor.psiElement ?: return
        val clause = ref.parent?.takeIf { it.node.elementType == E.IMPLEMENTS_CLAUSE } ?: return
        val type = PsiTreeUtil.getParentOfType(ref, JuxTypeDeclaration::class.java) ?: return
        JuxSupertypeClauseSurgery.move(
            project, type, ref, fromClause = clause,
            toClauseType = E.EXTENDS_CLAUSE, toKeyword = "extends",
        )
    }
}

/**
 * Shared clause-rewrite for the two move fixes. Works on the document (the
 * established quick-fix style here): removes the reference from its clause —
 * dropping the whole clause when it empties, or one comma otherwise — and
 * appends it to the target clause, creating `extends X` / `implements X`
 * right after the source clause's position when missing.
 */
internal object JuxSupertypeClauseSurgery {
    fun move(
        project: Project,
        type: JuxTypeDeclaration,
        ref: PsiElement,
        fromClause: PsiElement,
        toClauseType: com.intellij.psi.tree.IElementType,
        toKeyword: String,
    ) {
        val file = type.containingFile ?: return
        val docMgr = com.intellij.psi.PsiDocumentManager.getInstance(project)
        val doc = docMgr.getDocument(file) ?: return
        val refText = ref.text.trim()

        // --- removal range inside the source clause -------------------------
        val siblings = fromClause.children.filter { it.node.elementType == E.TYPE_REFERENCE }
        val removeStart: Int
        val removeEnd: Int
        if (siblings.size <= 1) {
            // Clause empties — remove it whole, including the leading space.
            removeStart = prefixWhitespaceStart(doc, fromClause.textRange.startOffset)
            removeEnd = fromClause.textRange.endOffset
        } else {
            val text = doc.charsSequence
            var start = ref.textRange.startOffset
            var end = ref.textRange.endOffset
            if (siblings.last() == ref) {
                // Trailing entry: eat the comma before it.
                while (start > 0 && text[start - 1].isWhitespace()) start--
                if (start > 0 && text[start - 1] == ',') start--
            } else {
                // Eat the comma (and spacing) after it.
                while (end < text.length && text[end].isWhitespace()) end++
                if (end < text.length && text[end] == ',') end++
                while (end < text.length && text[end] == ' ') end++
            }
            removeStart = start
            removeEnd = end
        }

        // --- insertion ------------------------------------------------------
        val target = type.node.findChildByType(toClauseType)?.psi
        val insertOffset: Int
        val insertText: String
        if (target != null && target != fromClause) {
            insertOffset = target.textRange.endOffset
            insertText = ", $refText"
        } else {
            // New clause where the old one sat (its slot is grammatical for
            // both orders only when extends precedes implements — for a new
            // `extends` insert before the implements clause; for a new
            // `implements` insert after the extends clause / removed range).
            insertOffset = if (toClauseType == E.EXTENDS_CLAUSE) {
                prefixWhitespaceStart(doc, fromClause.textRange.startOffset)
            } else {
                removeEnd
            }
            insertText = " $toKeyword $refText"
        }

        // Order the two edits so neither invalidates the other's offsets: an
        // insertion at/after the removed range goes first; an insertion at or
        // before its start happens after the deletion (which can't shift it).
        if (insertOffset >= removeEnd) {
            doc.insertString(insertOffset, insertText)
            doc.deleteString(removeStart, removeEnd)
        } else {
            doc.deleteString(removeStart, removeEnd)
            doc.insertString(insertOffset, insertText)
        }
        docMgr.commitDocument(doc)
    }

    /** Backtrack over the run of spaces/tabs before [offset]. */
    private fun prefixWhitespaceStart(doc: com.intellij.openapi.editor.Document, offset: Int): Int {
        var i = offset
        val text = doc.charsSequence
        while (i > 0 && (text[i - 1] == ' ' || text[i - 1] == '\t')) i--
        return i
    }
}
