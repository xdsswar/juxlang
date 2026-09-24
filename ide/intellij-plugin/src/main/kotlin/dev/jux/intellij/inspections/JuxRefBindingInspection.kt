package dev.jux.intellij.inspections

import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.LocalQuickFix
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemHighlightType
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.project.Project
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiElementVisitor
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.parser.JUX_REF_KW
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxType
import dev.jux.intellij.resolve.JuxTypeEngine

/**
 * The `ref` binding rules (JUX-MISSING-DEFS §M.13, ERRATA E84), mirrored from
 * the compiler so the editor and the build never disagree about them.
 *
 * `ref T` is a **binding mode**, not a type: it opts a binding of a VALUE type
 * (`String`, a primitive, a `struct`, a `record`, an enum) into the shared
 * reference semantics a class already has. The expression type of a `ref T`
 * binding stays plain `T` everywhere, which is why nothing here touches type
 * inference, completion, hover or go-to: those read the declaration's
 * `TYPE_REFERENCE`, and the parser consumes the `ref` keyword BEFORE that node
 * begins, so `ref String name` is a `String` to every other part of the plugin.
 *
 * Four rules, each carrying the compiler's own wording:
 *
 *  - **E0523** a `ref` RETURN type (`ref int f()`) is deferred by §M.13.2, so
 *    it is rejected outright.
 *  - **E0524** `ref ref T` nests a binding mode inside itself, which is not a
 *    thing: one `ref` already makes the binding shared.
 *  - **E0526** a `ref` generic argument (`Vec<ref int>`) puts a binding mode
 *    where a TYPE is wanted.
 *  - **W0490** `ref` on a binding whose type is ALREADY shared (a class, an
 *    interface, an array, a collection) is accepted and means nothing, so it
 *    is a warning naming the type, with a fix that removes the keyword.
 *  - **E0702** a `ref` binding captured by a `Worker.spawn` closure: the cell
 *    is task-local, so it cannot cross the thread boundary.
 *
 * The W0490 side is deliberately conservative. A false positive there is
 * worse than no warning at all, because `ref` on a value type is the ordinary,
 * correct use: a `String`, a `struct`, a `record`, an enum and every primitive
 * must stay silent, and so must a name the IDE cannot resolve (an unindexed
 * stub, a half-typed file) or a type parameter, whose argument is only known
 * where it is instantiated.
 */
class JuxRefBindingInspection : LocalInspectionTool() {

    override fun buildVisitor(holder: ProblemsHolder, isOnTheFly: Boolean): PsiElementVisitor =
        object : PsiElementVisitor() {
            override fun visitElement(element: PsiElement) {
                // `ref` only ever reaches the PSI once the compiler reserves the
                // keyword, which it now does; the null guard keeps the plugin
                // compiling against an older generated token registry.
                if (JUX_REF_KW == null) return
                when (element.elementType) {
                    // `Vec<ref int>` (§M.13.4): the angle-list parser keeps a
                    // `ref` there as a loose leaf, so it is visible here.
                    E.TYPE_ARGUMENT_LIST -> {
                        for (kw in refLeavesIn(element)) {
                            holder.registerProblem(
                                kw,
                                "a generic argument names a type, and `ref` is a binding mode, not a type -- " +
                                    "store the values and share the container, which is already a reference " +
                                    "type (§M.13.4) (E0526)",
                                ProblemHighlightType.GENERIC_ERROR,
                            )
                        }
                    }
                    E.LOCAL_VARIABLE, E.PARAMETER, E.FIELD_DECLARATION, E.METHOD_DECLARATION ->
                        checkDeclaration(element, holder)
                    // `Worker.spawn(() -> { … })` capturing a `ref` binding.
                    E.CALL_EXPRESSION -> checkSpawnCaptures(element, holder)
                    else -> {}
                }
            }
        }

    /** The `ref` rules that attach to one declaration: E0523, E0524, W0490. */
    private fun checkDeclaration(decl: PsiElement, holder: ProblemsHolder) {
        // A declaration's `ref` sits either directly under it (a local, a
        // parameter) or inside its modifier list (a field, a method), because
        // `ref` joined the declaration-modifier set in the parser.
        val refs = refLeavesIn(decl) +
            (decl.node.findChildByType(E.MODIFIER_LIST)?.psi?.let { refLeavesIn(it) } ?: emptyList())
        if (refs.isEmpty()) return

        // `ref ref T` (§M.13.4). The compiler reports once, on the SECOND
        // `ref`, and reads the rest of the declaration as if the extras were
        // not there; the range here is the same token.
        if (refs.size > 1) {
            holder.registerProblem(
                refs[1],
                "`ref` cannot nest: it is a binding mode, not a type, so one `ref` already makes the " +
                    "binding shared (§M.13.4) (E0524)",
                ProblemHighlightType.GENERIC_ERROR,
            )
        }

        // `ref int f()` -- a `ref` RETURN type, deferred by §M.13.2. A METHOD
        // declaration is the only place a `ref` modifier can mean the return
        // slot: on a field or a parameter the same keyword is the ordinary
        // binding mode.
        if (decl.elementType === E.METHOD_DECLARATION) {
            holder.registerProblem(
                refs[0],
                "a `ref` return type is not supported yet (§M.13.2) -- return the value, or take a " +
                    "`ref` parameter and write through it (E0523)",
                ProblemHighlightType.GENERIC_ERROR,
            )
            return
        }

        // W0490: the type is already a shared reference, so the `ref` adds
        // nothing. Only reported when the plugin is SURE what the type is.
        val typeRef = decl.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return
        val what = alreadySharedReason(typeRef) ?: return
        holder.registerProblem(
            refs[0],
            "`ref` adds nothing here: $what, so every binding of it shares one object (§M.13.2) (W0490)",
            ProblemHighlightType.WARNING,
            RemoveRefFix(),
        )
    }

    /**
     * Why the type written at [typeRef] is ALREADY a shared reference, phrased
     * as the compiler phrases it, or null when it is a value type (or when the
     * plugin cannot tell, which is treated the same way).
     *
     * The order matters and follows `warn_ref_on_reference_type`: an array
     * first (its element type is irrelevant, the handle is what is shared),
     * then `String`, which carries a stub CLASS for its members but is a VALUE
     * type per §M.13.1, then interfaces, then classes. A record, a struct, an
     * enum, a type parameter and every primitive fall through to null.
     */
    private fun alreadySharedReason(typeRef: PsiElement): String? {
        // A `String` reaches the type index as a stub class, so it has to be
        // ruled out by NAME before the type engine turns it into a ClassType.
        // Both spellings are the same primitive (grammar §A.1.3).
        if (baseTypeName(typeRef) in setOf("String", "string")) return null

        // `T?` is the same binding either way, so the nullable wrapper is
        // stripped exactly as the compiler strips it.
        val type = JuxTypeEngine.stripNullable(JuxTypeEngine.typeOfTypeReference(typeRef))
        return when (type) {
            // An array is a reference type (§6.5.2, ERRATA E84), so `ref int[]`
            // is the array spelling of this very warning, not an error.
            is JuxType.ArrayType -> "an array is already a reference type (§6.5.2)"
            is JuxType.ClassType -> {
                val decl = type.decl
                val name = decl.name ?: return null
                when {
                    isInterfaceDecl(decl) -> "`$name` is an interface, so a value of it is already a handle"
                    // A class, and the `rust.std` collections, which reach the
                    // index as stub classes. A `struct` is a VALUE type
                    // (ERRATA E20) and gets its own element type here, so it
                    // never lands in this branch.
                    decl.node.elementType === E.CLASS_DECLARATION -> "`$name` is already a reference type"
                    else -> null
                }
            }
            // A primitive, a type parameter, a function type, an unresolved
            // name: either a value type or unknown, and both stay silent.
            else -> null
        }
    }

    /** True for an `interface` declaration, read straight off the element type. */
    private fun isInterfaceDecl(decl: JuxTypeDeclaration): Boolean =
        decl.node.elementType === E.INTERFACE_DECLARATION

    /** The last identifier of a type reference's dotted name: `a.b.C[]` gives `C`. */
    private fun baseTypeName(typeRef: PsiElement): String? =
        typeRef.node.getChildren(null)
            .takeWhile { it.elementType !== E.TYPE_ARGUMENT_LIST && it.elementType !== T.LBRACKET }
            .lastOrNull { it.elementType === T.IDENTIFIER }
            ?.text

    /**
     * **E0702** for the `ref` case: a `Worker.spawn` closure runs on another OS
     * thread, and a `ref` binding is an `Rc`-backed cell that is task-local, so
     * it can never come along. Only the `ref` half of E0702 is mirrored here:
     * the class-capture half needs the whole `Send` analysis the checker does.
     */
    private fun checkSpawnCaptures(call: PsiElement, holder: ProblemsHolder) {
        val callee = call.firstChild ?: return
        // `Worker.spawn(…)`: the intrinsic has no other spelling (§18), and
        // matching on the written receiver rather than on a resolved symbol
        // keeps this working in a file the type engine cannot fully read.
        val calleeText = callee.text.trim()
        if (calleeText != "Worker.spawn") return
        val args = call.node.findChildByType(E.ARGUMENT_LIST)?.psi ?: return
        val lambda = PsiTreeUtil.findChildrenOfType(args, PsiElement::class.java)
            .firstOrNull { it.elementType === E.LAMBDA_EXPRESSION } ?: return

        // The `ref` bindings the enclosing function declares, by name. Read
        // from the declarations rather than through reference resolution: the
        // binding mode lives on the declaration, so this answers the question
        // directly and keeps working while the file is half-typed.
        val refNames = refBindingNamesAround(call)
        if (refNames.isEmpty()) return
        // The closure's own parameters shadow anything outside it, and so do
        // locals it declares itself, so neither is a capture.
        val shadowed = lambdaBoundNames(lambda)

        val reported = HashSet<String>()
        for (ref in PsiTreeUtil.findChildrenOfType(lambda, PsiElement::class.java)) {
            if (ref.elementType !== E.REFERENCE_EXPRESSION) continue
            // Only a BARE name is a capture: `other.field` reads through a
            // receiver, which the receiver's own name already accounts for.
            val name = ref.text.trim()
            if (name !in refNames || name in shadowed || !reported.add(name)) continue
            holder.registerProblem(
                ref,
                "`$name` cannot be captured by a `Worker.spawn` closure: a `ref` binding is a shared " +
                    "cell, which is task-local: copy the value into a local first, or share an " +
                    "`AtomicInt` (E0702)",
                ProblemHighlightType.GENERIC_ERROR,
            )
        }
    }

    /**
     * The names of every `ref` local and `ref` parameter declared by the
     * function (or free function) the element at [from] sits in. Fields are
     * left out on purpose: a `ref` field read inside a spawned closure is a
     * `this` capture, which the compiler reports with a different message and
     * a whole `Send` analysis behind it.
     */
    private fun refBindingNamesAround(from: PsiElement): Set<String> {
        val fn = generateSequence(from) { it.parent }
            .firstOrNull {
                it.elementType === E.METHOD_DECLARATION ||
                    it.elementType === E.CONSTRUCTOR_DECLARATION ||
                    it.parent == null
            } ?: return emptySet()
        val out = HashSet<String>()
        for (decl in PsiTreeUtil.findChildrenOfType(fn, PsiElement::class.java)) {
            if (decl.elementType !== E.LOCAL_VARIABLE && decl.elementType !== E.PARAMETER) continue
            if (refLeavesIn(decl).isEmpty()) continue
            (decl as? dev.jux.intellij.psi.JuxNamedElement)?.name?.let { out.add(it) }
        }
        return out
    }

    /** Every name a closure binds itself: its parameters and its own locals. */
    private fun lambdaBoundNames(lambda: PsiElement): Set<String> =
        PsiTreeUtil.findChildrenOfType(lambda, PsiElement::class.java)
            .filter { it.elementType === E.PARAMETER || it.elementType === E.LOCAL_VARIABLE }
            .mapNotNull { (it as? dev.jux.intellij.psi.JuxNamedElement)?.name }
            .toSet()

    /**
     * The `ref` keyword leaves that are DIRECT children of [parent].
     *
     * Read off the AST rather than `PsiElement.getChildren()`, which skips
     * leaf nodes: a keyword IS a leaf, so the PSI-level walk would find none
     * of them and every rule here would silently never fire.
     */
    private fun refLeavesIn(parent: PsiElement): List<PsiElement> =
        parent.node.getChildren(null)
            .filter { it.elementType === JUX_REF_KW }
            .map { it.psi }

    /**
     * Deletes the redundant `ref` keyword and the single space after it,
     * turning `ref Counter c` back into `Counter c`. Done through the document
     * rather than `PsiElement.delete()`: the keyword is a bare leaf, and
     * deleting one of those is not something every element type supports.
     */
    private class RemoveRefFix : LocalQuickFix {
        override fun getFamilyName(): String = "Remove the redundant `ref`"

        override fun applyFix(project: Project, descriptor: ProblemDescriptor) {
            val kw = descriptor.psiElement ?: return
            val file = kw.containingFile ?: return
            val doc = PsiDocumentManager.getInstance(project).getDocument(file) ?: return
            val start = kw.textRange.startOffset
            var end = kw.textRange.endOffset
            // Take the whitespace that separated `ref` from the type with it,
            // so the line does not keep a double space where the keyword was.
            val text = doc.charsSequence
            while (end < text.length && (text[end] == ' ' || text[end] == '\t')) end++
            doc.deleteString(start, end)
            PsiDocumentManager.getInstance(project).commitDocument(doc)
        }
    }
}
