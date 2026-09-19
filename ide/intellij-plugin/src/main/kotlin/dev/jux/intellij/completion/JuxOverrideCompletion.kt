package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.InsertionContext
import com.intellij.codeInsight.completion.PrioritizedLookupElement
import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.icons.AllIcons
import com.intellij.psi.PsiDocumentManager
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.codeInsight.JuxOverrideMembers
import dev.jux.intellij.psi.JuxTypeDeclaration
import dev.jux.intellij.resolve.JuxHierarchy

/**
 * Implement / override completion in a class body, as Java's editor has it:
 * typing the start of an inherited method's name where a member can begin
 * offers "area(): double  implement Shape", and accepting it writes the whole
 * member (`@override`, the signature with the supertype's type arguments
 * substituted, and a body), replacing any modifiers already typed on the line.
 *
 * The candidates, signatures and bodies are the ones Ctrl+I / Ctrl+O generate
 * ([JuxOverrideMembers]), so the two can never disagree. Methods to implement
 * rank above methods to override, as the class does not compile without them.
 */
object JuxOverrideCompletion {

    /** Modifiers that may already be typed before the name on the member's line. */
    private val TYPED_MODIFIERS = Regex("^((public|protected|private|internal|static|final|async)\\s+)*$")

    /** Adds one item per inheritable method the enclosing class has not declared. */
    fun addTo(parameters: CompletionParameters, add: (LookupElement) -> Unit) {
        val type = PsiTreeUtil.getParentOfType(parameters.position, JuxTypeDeclaration::class.java) ?: return
        // An interface declares contracts; it has nothing to implement.
        if (JuxHierarchy.isInterface(type)) return
        for (c in JuxOverrideMembers.candidates(type)) {
            val name = c.method.name ?: continue
            val implement = c.kind == JuxOverrideMembers.Kind.IMPLEMENT
            val params = JuxHierarchy.parameters(c.method).joinToString(", ") { it.text.replace(Regex("\\s+"), " ") }
            val element = LookupElementBuilder.create(c, name)
                .withIcon(if (implement) AllIcons.Gutter.ImplementingMethod else AllIcons.Gutter.OverridingMethod)
                .withPresentableText(name)
                .withTailText("($params)  ${if (implement) "implement" else "override"} ${c.ownerName}", true)
                .withTypeText(JuxHierarchy.returnTypeText(c.method))
                .withInsertHandler { ctx, _ -> insertMember(ctx, c) }
            add(PrioritizedLookupElement.withPriority(element, if (implement) 2.0 else 1.0))
        }
    }

    /**
     * Replaces the typed prefix (and any modifiers before it on the line) with
     * the full member, indented like the line it starts on.
     */
    private fun insertMember(ctx: InsertionContext, c: JuxOverrideMembers.Candidate) {
        val doc = ctx.document
        val line = doc.getLineNumber(ctx.startOffset)
        val lineStart = doc.getLineStartOffset(line)
        val before = doc.charsSequence.subSequence(lineStart, ctx.startOffset).toString()
        val indent = before.takeWhile { it == ' ' || it == '\t' }
        val typed = before.substring(indent.length)
        val from = if (TYPED_MODIFIERS.matches(typed)) lineStart + indent.length else ctx.startOffset
        // stubText starts with a newline and the indent; the line already has both.
        val text = JuxOverrideMembers.stubText(c, indent).removePrefix("\n$indent").trimEnd('\n')
        doc.replaceString(from, ctx.tailOffset, text)
        PsiDocumentManager.getInstance(ctx.project).commitDocument(doc)
        // Leave the caret inside the body, on the generated statement.
        val bodyLine = doc.getLineNumber(from) + 2
        if (bodyLine < doc.lineCount) {
            ctx.editor.caretModel.moveToOffset(doc.getLineEndOffset(bodyLine))
        }
    }
}
