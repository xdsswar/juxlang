package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.CompletionContributor
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionProvider
import com.intellij.codeInsight.completion.CompletionResultSet
import com.intellij.codeInsight.completion.CompletionType
import com.intellij.codeInsight.completion.PrioritizedLookupElement
import com.intellij.codeInsight.completion.util.ParenthesesInsertHandler
import com.intellij.codeInsight.lookup.LookupElement
import com.intellij.codeInsight.lookup.LookupElementBuilder
import com.intellij.icons.AllIcons
import com.intellij.ide.util.PropertiesComponent
import com.intellij.openapi.application.ApplicationManager
import com.intellij.openapi.extensions.ExtensionPointName
import com.intellij.patterns.PlatformPatterns
import com.intellij.psi.PsiElement
import com.intellij.psi.util.PsiTreeUtil
import com.intellij.psi.util.elementType
import com.intellij.util.ProcessingContext
import dev.jux.intellij.JuxLanguage
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxObservableProps
import dev.jux.intellij.psi.JuxPropertyDeclaration
import javax.swing.Icon

/**
 * **Fallback** IDE-side completion: contextual keywords plus the declarations
 * *visible from the caret* — for IDEs running without any LSP client (no
 * native LSP module and no LSP4IJ).
 *
 * When `juxc-lsp` IS serving the project (native client or LSP4IJ), this
 * contributor bails out entirely: the server's completions are scope-aware
 * (locals, parameters, visibility-filtered members, ranked), and merging this
 * flat list on top would duplicate labels and push the good items down.
 *
 * Relevance contract (most relevant on top, least below — enforced through
 * [PrioritizedLookupElement] tiers, see the `P_*` constants):
 *
 *  1. locals declared before the caret + enclosing parameters (and the
 *     implicit `value` in a setter body),
 *  2. members of the enclosing class — fields, properties, methods,
 *  3. position-legal keywords ([JuxKeywordContext] — `class` is never offered
 *     inside a method body, `return` never at the top level),
 *  4. file-level type names.
 *
 * Nothing else is offered: locals of OTHER methods and members of OTHER
 * classes are unreachable from the caret and would only be noise. After a
 * `.` the receiver's members belong to `juxc-lsp`; the single exception is the
 * §P property surface (`.observers` + its ops, `bind`/`unbind`/
 * `bindBidirectional`), which the plugin owns structurally.
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

        // Relevance tiers (higher floats to the top of the lookup).
        const val P_LOCAL = 100.0
        const val P_PARAM = 90.0
        const val P_MEMBER = 80.0
        const val P_KEYWORD = 60.0
        const val P_TYPE = 50.0
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

                    // After a `.` (member access) the receiver's members come
                    // from `juxc-lsp` — except the §P property surface, which
                    // is structural and plugin-owned.
                    if (isAfterDot(parameters)) {
                        addPropertySurface(parameters, result)
                        return
                    }

                    // Tier 3: only the keywords the grammar accepts here.
                    for (kw in JuxKeywordContext.keywordsFor(parameters.position)) {
                        result.addElement(
                            ranked(LookupElementBuilder.create(kw).bold(), P_KEYWORD),
                        )
                    }

                    // Tiers 1, 2, 4: declarations visible from the caret only.
                    addVisibleDeclarations(parameters, result)
                }
            },
        )
    }

    // ---- visible-declaration tiers ------------------------------------------

    /**
     * Walks the enclosing scopes from the caret out, offering exactly what an
     * identifier here could legally name — same shape as
     * [dev.jux.intellij.resolve.JuxReference.resolveLocally], with relevance
     * falling as the scope widens.
     */
    private fun addVisibleDeclarations(parameters: CompletionParameters, result: CompletionResultSet) {
        val offset = parameters.offset
        val seen = HashSet<String>()
        fun add(element: LookupElement, name: String) {
            if (seen.add(name)) result.addElement(element)
        }

        var scope: PsiElement? = parameters.position.parent
        while (scope != null && scope !is JuxFile) {
            when (scope.elementType) {
                E.CODE_BLOCK ->
                    // Locals are visible only after their declaration.
                    for (child in scope.children) {
                        if (child.elementType !== E.LOCAL_VARIABLE) continue
                        if (child.textOffset >= offset) continue
                        val named = child as? JuxNamedElement ?: continue
                        val name = named.name ?: continue
                        add(declaration(named, name, AllIcons.Nodes.Variable, P_LOCAL), name)
                    }
                E.METHOD_DECLARATION, E.CONSTRUCTOR_DECLARATION, E.OPERATOR_DECLARATION ->
                    scope.children.firstOrNull { it.elementType === E.PARAMETER_LIST }
                        ?.children?.forEach { p ->
                            if (p.elementType !== E.PARAMETER) return@forEach
                            val name = (p as? JuxNamedElement)?.name ?: return@forEach
                            add(declaration(p, name, AllIcons.Nodes.Parameter, P_PARAM), name)
                        }
                // Inside a setter body, the implicit `value` parameter (§P.1.4).
                E.PROPERTY_ACCESSOR ->
                    if (firstIdentifierText(scope) == "set") {
                        add(
                            ranked(
                                LookupElementBuilder.create(JuxObservableProps.SETTER_VALUE)
                                    .withIcon(AllIcons.Nodes.Parameter)
                                    .withTypeText("setter value", true),
                                P_PARAM,
                            ),
                            JuxObservableProps.SETTER_VALUE,
                        )
                    }
                E.CLASS_BODY ->
                    for (m in scope.children) {
                        val named = m as? JuxNamedElement ?: continue
                        val name = named.name ?: continue
                        when (m.elementType) {
                            E.FIELD_DECLARATION, E.CONST_DECLARATION ->
                                add(declaration(named, name, AllIcons.Nodes.Field, P_MEMBER), name)
                            E.PROPERTY_DECLARATION ->
                                add(declaration(named, name, AllIcons.Nodes.Property, P_MEMBER), name)
                            E.METHOD_DECLARATION ->
                                add(method(named, name), name)
                            else -> {}
                        }
                    }
                else -> {}
            }
            scope = scope.parent
        }

        // Tier 4: file-level type names (`Model m = new Model();`).
        val file = parameters.originalFile
        for (decl in file.children) {
            val named = decl as? JuxNamedElement ?: continue
            val name = named.name ?: continue
            if (decl.elementType in TYPE_DECLS) {
                add(declaration(named, name, AllIcons.Nodes.Class, P_TYPE), name)
            }
        }
    }

    /** A non-method declaration lookup: icon + declared-type hint + tier. */
    private fun declaration(decl: PsiElement, name: String, icon: Icon, priority: Double): LookupElement {
        val typeText = decl.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()
        var builder = LookupElementBuilder.create(name).withIcon(icon)
        if (typeText != null) builder = builder.withTypeText(typeText, true)
        return ranked(builder, priority)
    }

    /** A method lookup: parens inserted on selection, caret between them. */
    private fun method(decl: JuxNamedElement, name: String): LookupElement {
        val params = (decl as PsiElement).node.findChildByType(E.PARAMETER_LIST)?.text ?: "()"
        val returnType = decl.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()
        var builder = LookupElementBuilder.create(name)
            .withIcon(AllIcons.Nodes.Method)
            .withTailText(params.replace(Regex("\\s+"), " "), true)
            .withInsertHandler(ParenthesesInsertHandler.getInstance(params != "()"))
        if (returnType != null) builder = builder.withTypeText(returnType, true)
        return ranked(builder, P_MEMBER)
    }

    // ---- §P property surface after `.` ----------------------------------------

    /**
     * The one member-access surface the plugin owns without the LSP: on
     * `<prop>.` offer `observers` and the binding ops; on `<prop>.observers.`
     * offer attach/detach (with parens) and clear/size (paren-free, §P.3.2).
     * Only fires when the receiver word is `observers` or matches a property
     * declared in this file — anything else stays empty for `juxc-lsp`.
     */
    private fun addPropertySurface(parameters: CompletionParameters, result: CompletionResultSet) {
        val word = wordBeforeDot(parameters) ?: return

        if (word == JuxObservableProps.OBSERVERS_MEMBER) {
            for (op in JuxObservableProps.OBSERVERS_OPS) {
                val parenFree = op in JuxObservableProps.PAREN_FREE_OPS
                var b = LookupElementBuilder.create(op)
                    .withIcon(AllIcons.Nodes.Method)
                    .withTypeText("observers", true)
                if (!parenFree) b = b.withInsertHandler(ParenthesesInsertHandler.WITH_PARAMETERS)
                result.addElement(ranked(b, P_LOCAL))
            }
            return
        }

        // `<prop>.` — only when the receiver names a property in this file.
        val isProperty = PsiTreeUtil
            .findChildrenOfType(parameters.originalFile, JuxPropertyDeclaration::class.java)
            .any { it.name == word }
        if (!isProperty) return

        result.addElement(
            ranked(
                LookupElementBuilder.create(JuxObservableProps.OBSERVERS_MEMBER)
                    .withIcon(AllIcons.Nodes.Property)
                    .withTypeText("observable", true),
                P_LOCAL,
            ),
        )
        for (op in JuxObservableProps.BIND_OPS) {
            result.addElement(
                ranked(
                    LookupElementBuilder.create(op)
                        .withIcon(AllIcons.Nodes.Method)
                        .withTypeText("binding", true)
                        .withInsertHandler(ParenthesesInsertHandler.getInstance(op != "unbind")),
                    P_PARAM,
                ),
            )
        }
    }

    /** The identifier word immediately before the `.` the caret follows, or null. */
    private fun wordBeforeDot(parameters: CompletionParameters): String? {
        val text = parameters.editor.document.charsSequence
        var i = parameters.offset - 1
        while (i >= 0 && (text[i].isLetterOrDigit() || text[i] == '_')) i--
        while (i >= 0 && text[i].isWhitespace()) i--
        if (i < 0 || text[i] != '.') return null
        var j = i - 1
        while (j >= 0 && (text[j].isLetterOrDigit() || text[j] == '_')) j--
        val word = text.subSequence(j + 1, i).toString()
        return word.ifEmpty { null }
    }

    // ---- shared plumbing --------------------------------------------------------

    private fun ranked(builder: LookupElementBuilder, priority: Double): LookupElement =
        PrioritizedLookupElement.withPriority(builder, priority)

    private fun firstIdentifierText(scope: PsiElement): String? {
        var c: PsiElement? = scope.firstChild
        while (c != null) {
            if (c.elementType === dev.jux.intellij.highlight.JuxTokenTypes.IDENTIFIER) return c.text
            c = c.nextSibling
        }
        return null
    }

    private val TYPE_DECLS = setOf(
        E.CLASS_DECLARATION, E.INTERFACE_DECLARATION, E.ENUM_DECLARATION,
        E.RECORD_DECLARATION, E.STRUCT_DECLARATION, E.ANNOTATION_DECLARATION,
        E.TYPE_ALIAS_DECLARATION,
    )

    /**
     * True when the caret sits in a member-access position — i.e. the nearest
     * non-identifier, non-whitespace char before the (possibly partial) name
     * being completed is a `.`.
     */
    private fun isAfterDot(parameters: CompletionParameters): Boolean {
        val text = parameters.editor.document.charsSequence
        var i = parameters.offset - 1
        while (i >= 0 && (text[i].isLetterOrDigit() || text[i] == '_')) i--
        while (i >= 0 && text[i].isWhitespace()) i--
        return i >= 0 && text[i] == '.'
    }

    /**
     * True when an LSP client (native or LSP4IJ) is serving `juxc-lsp`
     * completions for this project, so the fallback items would only
     * duplicate them.
     *
     * Classloading discipline: the native check goes through
     * [dev.jux.intellij.lsp.JuxNativeLspStatus] ONLY behind the
     * `platform.lsp.serverSupportProvider` extension-point probe (those
     * classes exist exactly when the EP does); the LSP4IJ check never touches
     * LSP4IJ classes at all — plugin presence plus the persisted enable flag
     * are enough.
     */
    private fun lspProvidesCompletion(parameters: CompletionParameters): Boolean {
        val app = ApplicationManager.getApplication()
        // The headless test fixture never starts a server — keep the fallback
        // alive so completion tests exercise it.
        if (app.isUnitTestMode) return false
        val project = parameters.position.project
        // Native client: only when OUR provider is registered AND a Jux server
        // session is actually up. NOT an early-return false — on 2024.2–2025.1
        // paid IDEs the EP exists but our lsp.xml never loaded, and LSP4IJ may
        // be serving instead. (A registered Jux provider implies the platform
        // LSP classes exist, so touching JuxNativeLspStatus is safe.)
        val nativeRegistered = NATIVE_LSP_EP.extensionsIfPointIsRegistered
            .any { it.javaClass.name == JUX_NATIVE_PROVIDER }
        if (nativeRegistered &&
            dev.jux.intellij.lsp.JuxNativeLspStatus.isActive(project)
        ) {
            return true
        }
        // LSP4IJ path: probe its `server` extension point — it has
        // registrations exactly when the LSP4IJ plugin is installed and
        // enabled (and then our lsp4ij.xml loaded too). Avoids the internal
        // PluginManagerCore API and the PluginId.Companion field that doesn't
        // exist before 2025.2. Additionally require the user's server toggle
        // on AND a real `juxc-lsp` binary — a fresh machine without the
        // toolchain must keep the fallback completions.
        // LOCKSTEP: the key mirrors JuxLsp4ijServerFactory.ENABLED_KEY
        // (not importable here — classloading firewall).
        if (LSP4IJ_SERVER_EP.extensionsIfPointIsRegistered.isEmpty()) return false
        if (!dev.jux.intellij.run.JuxLspCommandLine.isResolvable()) return false
        return PropertiesComponent.getInstance(project)
            .getBoolean("dev.jux.lsp4ij.enabled", true)
    }
}
