package dev.jux.intellij.inspections

import com.intellij.codeInsight.daemon.HighlightDisplayKey
import com.intellij.codeInspection.InspectionManager
import com.intellij.codeInspection.LocalInspectionTool
import com.intellij.codeInspection.ProblemDescriptor
import com.intellij.codeInspection.ProblemDescriptorBase
import com.intellij.codeInspection.ProblemsHolder
import com.intellij.openapi.editor.Document
import com.intellij.openapi.progress.ProcessCanceledException
import com.intellij.openapi.project.DumbService
import com.intellij.openapi.util.Key
import com.intellij.openapi.util.TextRange
import com.intellij.profile.codeInspection.InspectionProjectProfileManager
import com.intellij.psi.PsiFile
import com.intellij.psi.util.CachedValue
import com.intellij.psi.util.CachedValueProvider
import com.intellij.psi.util.CachedValuesManager
import com.intellij.psi.util.PsiModificationTracker
import com.intellij.psi.util.PsiTreeUtil
import dev.jux.intellij.highlight.JuxSyntaxHintAnnotator
import dev.jux.intellij.psi.JuxFile

/**
 * The compiler diagnostics the plugin reports ITSELF, and the rule that keeps
 * the language server from reporting them a second time.
 *
 * The hybrid engine runs two sources of red: the plugin's own inspections,
 * which carry quick-fixes built on the PSI ("Implement methods", "Import
 * 'beta.Widget'", "Move to 'extends'"), and `juxc-lsp`, which publishes what
 * the real checker finds. Where both find the same mistake the user saw it
 * twice. So an LSP diagnostic is dropped when the plugin reports the SAME code
 * over an overlapping range in the same file -- and only then:
 *
 *  - the plugin's checks are deliberately conservative (silent on a name they
 *    cannot resolve), so a code being mirrored does not mean the plugin found
 *    every instance of it. Dropping by code alone would hide real compiler
 *    errors; dropping by code AND place hides only the duplicate.
 *  - `E0410` and `E0301` are broad compiler codes the plugin covers only a
 *    corner of (a comparator's result, a Java habit, an unresolved bare
 *    name); every other `E0410` still comes from the server.
 *  - an inspection the user switched off reports nothing, so the server's
 *    copy is shown.
 *
 * The plugin's copy is the one kept because it is the one with the PSI
 * quick-fixes. [MIRRORED] is the single list both sides read: an inspection
 * that starts reporting a new code must be added here, and
 * `JuxMirroredDiagnosticsTest` fails when a `(E0xxx)` in an inspection's text
 * is missing from it.
 */
object JuxMirroredDiagnostics {

    /** One plugin-side source of compiler codes. */
    class Source(
        /** The inspection's `shortName`, or null for the always-on annotator. */
        val shortName: String?,
        /** Every code its messages carry. A message with none counts as [codes]' only entry. */
        val codes: Set<String>,
        /** Creates the inspection to run it for [pluginReports]; null for the annotator. */
        val create: (() -> LocalInspectionTool)?,
    )

    /** Every plugin check that mirrors a compiler diagnostic, by inspection. */
    val SOURCES: List<Source> = listOf(
        Source("JuxAbiRules", setOf("E0519", "E0520", "E0521", "E0522", "E0527", "E0950", "E0951")) { JuxAbiRulesInspection() },
        Source("JuxAbstractNotImplemented", setOf("E0429")) { JuxAbstractNotImplementedInspection() },
        Source("JuxAccessorVisibility", setOf("E0972")) { JuxAccessorVisibilityInspection() },
        Source("JuxAmbiguousImport", setOf("E0303")) { JuxAmbiguousImportInspection() },
        Source("JuxBindTypeMismatch", setOf("E0974")) { JuxBindTypeMismatchInspection() },
        Source("JuxBoundPropertyAssignment", setOf("E0973")) { JuxBoundPropertyAssignmentInspection() },
        Source("JuxComparatorResult", setOf("E0410")) { JuxComparatorResultInspection() },
        Source("JuxDeprecatedUsage", setOf("W0491")) { JuxDeprecatedUsageInspection() },
        Source("JuxExtendsClause", setOf("E0420", "E0422", "E0423")) { JuxExtendsClauseInspection() },
        Source("JuxFinalReassignment", setOf("E0464")) { JuxFinalReassignmentInspection() },
        Source("JuxForeignBounds", setOf("E0446")) { JuxForeignBoundsInspection() },
        Source("JuxFunctionTypeVariance", setOf("E0410")) { JuxFunctionTypeVarianceInspection() },
        Source("JuxGenerator", setOf("E0990", "E0994", "E0995", "E0996", "E0997")) { JuxGeneratorInspection() },
        Source("JuxImplementsClause", setOf("E0424")) { JuxImplementsClauseInspection() },
        Source("JuxInterfaceOperator", setOf("E0936")) { JuxInterfaceOperatorInspection() },
        Source("JuxJavaHabit", setOf("E0301", "E0413")) { JuxJavaHabitInspection() },
        Source("JuxNeverFunction", setOf("E0485", "E0486")) { JuxNeverFunctionInspection() },
        Source("JuxPropertyNaming", setOf("W0974")) { JuxPropertyNamingInspection() },
        Source("JuxPropertyNeverObserved", setOf("W0971")) { JuxPropertyNeverObservedInspection() },
        Source("JuxRefBinding", setOf("E0523", "E0524", "E0526", "E0702", "W0490")) { JuxRefBindingInspection() },
        Source("JuxSafetyComment", setOf("W0820")) { JuxSafetyCommentInspection() },
        Source("JuxSetterEarlyReturn", setOf("W0973")) { JuxSetterEarlyReturnInspection() },
        Source("JuxTimeSpan", setOf("E0487")) { JuxTimeSpanInspection() },
        Source("JuxTypeAlias", setOf("E0443", "E0498")) { JuxTypeAliasInspection() },
        Source("JuxTypeArgumentCount", setOf("E0443")) { JuxTypeArgumentCountInspection() },
        Source("JuxTypeParameterBound", setOf("E0419", "E0459")) { JuxTypeParameterBoundInspection() },
        Source("JuxUninferableLambda", setOf("E0453")) { JuxUninferableLambdaInspection() },
        // "Cannot resolve symbol / type" is the compiler's E0301 without the code in its text.
        Source("JuxUnresolvedReference", setOf("E0301")) { JuxUnresolvedReferenceInspection() },
        // The syntax-hint annotator's keyword-as-binding-name check.
        Source(null, setOf("E0204"), null),
    )

    /** Every code the plugin reports itself. */
    val MIRRORED: Set<String> = SOURCES.flatMapTo(HashSet()) { it.codes }

    /**
     * True when the language server's diagnostic [code] over [range] of [file]
     * is one the plugin already shows: the same code, reported by the plugin
     * over an overlapping range. False whenever that cannot be established.
     */
    fun isDuplicate(file: PsiFile?, code: String?, range: TextRange?): Boolean {
        if (file !is JuxFile || code == null || range == null || code !in MIRRORED) return false
        if (DumbService.isDumb(file.project)) return false
        return try {
            pluginReports(file).any { it.code == code && it.range.intersects(range) }
        } catch (e: ProcessCanceledException) {
            throw e
        } catch (_: Throwable) {
            // Never lose a server diagnostic because the plugin's check failed.
            false
        }
    }

    /** The code of an LSP diagnostic: its `code` field, else a `[E0424]` in its message. */
    fun codeOf(code: String?, message: String?): String? =
        code?.takeIf { CODE.matches(it) } ?: message?.let { BRACKETED.find(it)?.groupValues?.get(1) }

    /** An LSP position range as an offset range of [document], clamped to it. */
    fun textRange(document: Document, startLine: Int, startChar: Int, endLine: Int, endChar: Int): TextRange? {
        if (document.lineCount == 0) return TextRange(0, 0)
        fun offset(line: Int, ch: Int): Int {
            val l = line.coerceIn(0, document.lineCount - 1)
            return (document.getLineStartOffset(l) + ch).coerceAtMost(document.getLineEndOffset(l))
        }
        val start = offset(startLine, startChar)
        val end = offset(endLine, endChar)
        return if (end < start) null else TextRange(start, end)
    }

    /** One diagnostic the plugin reports: its code and where. */
    data class Report(val code: String, val range: TextRange)

    /**
     * What the plugin itself reports in [file], from every enabled mirroring
     * inspection and the annotator. The raw per-inspection results are cached
     * until the PSI changes; whether each inspection is on is asked each time,
     * so switching one off brings the server's copy back at once.
     */
    fun pluginReports(file: JuxFile): List<Report> {
        val raw = CachedValuesManager.getManager(file.project).getCachedValue(file, REPORTS_KEY, {
            CachedValueProvider.Result.create(computeReports(file), PsiModificationTracker.MODIFICATION_COUNT)
        }, false)
        val profile = InspectionProjectProfileManager.getInstance(file.project).currentProfile
        return SOURCES.flatMap { source ->
            val enabled = source.shortName == null ||
                HighlightDisplayKey.find(source.shortName)?.let { profile.isToolEnabled(it, file) } == true
            if (enabled) raw[source].orEmpty() else emptyList()
        }
    }

    private val REPORTS_KEY: Key<CachedValue<Map<Source, List<Report>>>> = Key.create("jux.mirrored.reports")

    private fun computeReports(file: JuxFile): Map<Source, List<Report>> {
        val manager = InspectionManager.getInstance(file.project)
        val out = HashMap<Source, List<Report>>()
        for (source in SOURCES) {
            val create = source.create
            out[source] = if (create == null) annotatorReports(file) else run {
                val holder = ProblemsHolder(manager, file, false)
                val visitor = create().buildVisitor(holder, false)
                PsiTreeUtil.processElements(file) { it.accept(visitor); true }
                holder.results.mapNotNull { report(it, source) }
            }
        }
        return out
    }

    private fun annotatorReports(file: JuxFile): List<Report> {
        val out = ArrayList<Report>()
        PsiTreeUtil.processElements(file) { e ->
            JuxSyntaxHintAnnotator.keywordBindingName(e)?.let { out.add(Report("E0204", it.textRange)) }
            true
        }
        return out
    }

    private fun report(d: ProblemDescriptor, source: Source): Report? {
        val code = CODE_IN_TEXT.find(d.descriptionTemplate)?.groupValues?.get(1)
            ?: source.codes.singleOrNull()
            ?: return null
        val range = (d as? ProblemDescriptorBase)?.textRange ?: d.psiElement?.textRange ?: return null
        return Report(code, range)
    }

    private val CODE = Regex("""[EW]\d{4}""")
    private val CODE_IN_TEXT = Regex("""\(([EW]\d{4})\)""")
    private val BRACKETED = Regex("""\[([EW]\d{4})]""")
}
