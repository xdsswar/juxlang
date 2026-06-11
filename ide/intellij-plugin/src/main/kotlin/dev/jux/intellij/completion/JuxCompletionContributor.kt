package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.CompletionContributor
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionProvider
import com.intellij.codeInsight.completion.CompletionResultSet
import com.intellij.codeInsight.completion.CompletionType
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.ide.util.PropertiesComponent
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.extensions.ExtensionPointName
import com.intellij.patterns.PlatformPatterns
import com.intellij.psi.search.PsiElementProcessor
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.util.ProcessingContext
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.highlight.JuxKeywords
import dev.jux.intellij.psi.JuxNamedElement

/**
 * **Fallback** IDE-side completion: reserved keywords plus the named
 * declarations in the current file — for IDEs running without any LSP client
 * (no native LSP module and no LSP4IJ).
 *
 * When `juxc-lsp` IS serving the project (native client or LSP4IJ), this
 * contributor bails out entirely: the server's completions are scope-aware
 * (locals, parameters, visibility-filtered members, ranked), and merging this
 * flat list on top would duplicate labels and push the good items down.
 */
class JuxCompletionContributor : CompletionContributor() {
    private companion object {
        /**
         * EP handles addressed by name — public [ExtensionPointName] API
         * (`extensionsIfPointIsRegistered` returns an empty list when the EP
         * doesn't exist, loading nothing). The `ExtensionsArea` route is
         * internal API on 2024.2.
         */
        val NATIVE_LSP_EP: ExtensionPointName<Any> =
            ExtensionPointName.create("com.intellij.platform.lsp.serverSupportProvider")
        val LSP4IJ_SERVER_EP: ExtensionPointName<Any> =
            ExtensionPointName.create("com.redhat.devtools.lsp4ij.server")

        /** LOCKSTEP: mirrors `lsp.xml`'s `implementationClass`. */
        const val JUX_NATIVE_PROVIDER = "dev.jux.intellij.lsp.JuxLspServerSupportProvider"
    }

    init {
        extend(
            CompletionType.BASIC,
            PlatformPatterns.psiElement().withLanguage(JuxLanguage),
            object : CompletionProvider<CompletionParameters>() {
                override fun addCompletions(
                    parameters: CompletionParameters,
                    context: ProcessingContext,
                    result: CompletionResultSet,
                ) {
                    // An active LSP session supplies smarter versions of
                    // everything this contributor offers — stand down.
                    if (lspProvidesCompletion(parameters)) return

                    // After a `.` (member access) the only relevant completions
                    // are the receiver's members, which come from `juxc-lsp`.
                    // Contributing keywords/declarations here would push the
                    // class members down the list (the user has to scroll past
                    // `for`/`if`/… to reach them), so bail out entirely.
                    if (isAfterDot(parameters)) return

                    // Only the keywords the grammar accepts at this position
                    // (statements in a block, members in a class body, …).
                    // `position` is the dummy-identifier leaf in the completion
                    // copy — always present and parsed, even in an empty file.
                    val keywords = JuxKeywordContext.keywordsFor(parameters.position)
                    for (kw in keywords) {
                        result.addElement(LookupElementBuilder.create(kw).bold())
                    }
                    val seen = HashSet<String>()
                    PsiTreeUtil.processElements(parameters.originalFile, PsiElementProcessor { e ->
                        if (e is JuxNamedElement) e.name?.let { if (seen.add(it)) result.addElement(LookupElementBuilder.create(it)) }
                        true
                    })
                }

                /**
                 * True when the caret sits in a member-access position — i.e.
                 * the nearest non-identifier, non-whitespace char before the
                 * (possibly partial) name being completed is a `.`.
                 */
                private fun isAfterDot(parameters: CompletionParameters): Boolean {
                    val text = parameters.editor.document.charsSequence
                    var i = parameters.offset - 1
                    while (i >= 0 && (text[i].isLetterOrDigit() || text[i] == '_')) i--
                    while (i >= 0 && text[i].isWhitespace()) i--
                    return i >= 0 && text[i] == '.'
                }

                /**
                 * True when an LSP client (native or LSP4IJ) is serving
                 * `juxc-lsp` completions for this project, so the fallback
                 * items would only duplicate them.
                 *
                 * Classloading discipline: the native check goes through
                 * [dev.jux.intellij.lsp.JuxNativeLspStatus] ONLY behind the
                 * `platform.lsp.serverSupportProvider` extension-point probe
                 * (those classes exist exactly when the EP does); the LSP4IJ
                 * check never touches LSP4IJ classes at all — plugin presence
                 * plus the persisted enable flag are enough.
                 */
                private fun lspProvidesCompletion(parameters: CompletionParameters): Boolean {
                    val app = ApplicationManager.getApplication()
                    // The headless test fixture never starts a server — keep
                    // the fallback alive so completion tests exercise it.
                    if (app.isUnitTestMode) return false
                    val project = parameters.position.project
                    // Native client: only when OUR provider is registered AND
                    // a Jux server session is actually up. NOT an early-return
                    // false — on 2024.2–2025.1 paid IDEs the EP exists but our
                    // lsp.xml never loaded, and LSP4IJ may be serving instead.
                    // (A registered Jux provider implies the platform LSP
                    // classes exist, so touching JuxNativeLspStatus is safe.)
                    val nativeRegistered = NATIVE_LSP_EP.extensionsIfPointIsRegistered
                        .any { it.javaClass.name == JUX_NATIVE_PROVIDER }
                    if (nativeRegistered &&
                        dev.jux.intellij.lsp.JuxNativeLspStatus.isActive(project)
                    ) {
                        return true
                    }
                    // LSP4IJ path: probe its `server` extension point — it has
                    // registrations exactly when the LSP4IJ plugin is installed
                    // and enabled (and then our lsp4ij.xml loaded too). Avoids
                    // the internal PluginManagerCore API and the
                    // PluginId.Companion field that doesn't exist before
                    // 2025.2. Additionally require the user's server toggle on
                    // AND a real `juxc-lsp` binary — a fresh machine without
                    // the toolchain must keep the fallback completions.
                    // LOCKSTEP: the key mirrors JuxLsp4ijServerFactory.ENABLED_KEY
                    // (not importable here — classloading firewall).
                    if (LSP4IJ_SERVER_EP.extensionsIfPointIsRegistered.isEmpty()) return false
                    if (!dev.jux.intellij.run.JuxLspCommandLine.isResolvable()) return false
                    return PropertiesComponent.getInstance(project)
                        .getBoolean("dev.jux.lsp4ij.enabled", true)
                }
            },
        )
    }
}
