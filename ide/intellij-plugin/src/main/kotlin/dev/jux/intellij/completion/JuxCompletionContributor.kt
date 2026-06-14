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
                    // Inside a `$"…${ ⟨caret⟩ }…"` interpolation hole the LSP is
                    // blind — the whole literal is one opaque string token to it,
                    // so it never completes there. The plugin therefore OWNS hole
                    // completion and runs it even when the LSP is serving the rest
                    // of the file. Must come before the LSP early-return below.
                    if (addInterpHoleCompletion(parameters, result)) return

                    // An active LSP session supplies smarter versions of
                    // everything this contributor offers — stand down. (The
                    // probe only reports "active" when juxc-lsp can ACTUALLY
                    // serve — toolchain resolvable + session up — so a missing
                    // or broken toolchain never silences the fallback.)
                    if (lspProvidesCompletion(parameters)) return

                    // After a `.` (member access): offer the receiver's real
                    // members (methods/fields/properties/enum constants of an
                    // in-file-resolvable type — see JuxTypeInference) plus the
                    // §P property surface. Without the LSP this is the only
                    // member completion the user gets, so it must be solid.
                    if (isAfterDot(parameters)) {
                        addMemberCompletion(parameters, result)
                        addPropertySurface(parameters, result)
                        return
                    }

                    // After `@` — only the builtin annotations exist in Phase 1
                    // (`@override` + the §TS.1 test/hook five), so nothing else
                    // belongs in the list.
                    if (isAfterAt(parameters)) {
                        addBuiltinAnnotations(result)
                        return
                    }

                    // Tier 3: only the keywords the grammar accepts here.
                    val keywords = JuxKeywordContext.keywordsFor(parameters.position)
                    for (kw in keywords) {
                        result.addElement(
                            ranked(LookupElementBuilder.create(kw).bold(), P_KEYWORD),
                        )
                    }
                    // `for await (…)` (§18.6) — a two-word statement opener, so
                    // it can't ride the curated single-word sets (same reason
                    // OBSERVER joins them as a raw extra in JuxKeywordContext).
                    if (keywords === JuxKeywordContext.STATEMENT) {
                        result.addElement(
                            ranked(LookupElementBuilder.create("for await").bold(), P_KEYWORD),
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

        // Tier 4: file-level type names (`Model m = new Model();`) — no import.
        val file = parameters.originalFile
        // A native fn is only callable inside an `unsafe` block (E0506 elsewhere),
        // so only offer them when the caret is within one — otherwise they'd be
        // wrong-scope noise ranked above the user's own types.
        val insideUnsafe = run {
            var p: PsiElement? = parameters.position
            while (p != null && p !is JuxFile) {
                if (p.elementType === E.UNSAFE_STATEMENT) return@run true
                p = p.parent
            }
            false
        }
        for (decl in file.children) {
            // §L.7 C-FFI: surface a `native { … }` block's foreign functions as
            // file-level callables, but only inside `unsafe` (see above).
            if (decl.elementType === E.EXTERN_BLOCK) {
                if (insideUnsafe) {
                    for (fn in decl.children) {
                        if (fn.elementType !== E.METHOD_DECLARATION) continue
                        val named = fn as? JuxNamedElement ?: continue
                        val name = named.name ?: continue
                        add(method(named, name), name)
                    }
                }
                continue
            }
            val named = decl as? JuxNamedElement ?: continue
            val name = named.name ?: continue
            if (decl.elementType in TYPE_DECLS) {
                add(declaration(named, name, AllIcons.Nodes.Class, P_TYPE), name)
            }
        }

        // Tier 4b: project-wide types from OTHER files — discoverable here with
        // auto-import on accept. This is what lets cross-file types show up
        // without the LSP; the slightly-lower priority keeps in-file names on
        // top. (Rust std / crate types come from the LSP's stub index.)
        val project = parameters.position.project
        // The project type index reads FileTypeIndex, which throws
        // IndexNotReadyException during indexing (dumb mode). Completion can fire
        // then (e.g. right after open / a big VCS update), so skip the cross-file
        // walk rather than abort the whole popup; in-file names above still show.
        if (com.intellij.openapi.project.DumbService.isDumb(project)) return
        val curPkg = dev.jux.intellij.completion.JuxAutoImport.packageOfFile(file)
        dev.jux.intellij.resolve.JuxTypeIndex.forEachType(
            project,
            com.intellij.psi.search.GlobalSearchScope.allScope(project),
        ) { type ->
            val name = type.name
            if (name != null && name !in seen) {
                val pkg = dev.jux.intellij.completion.JuxAutoImport.packageOf(type)
                var b = LookupElementBuilder.create(name).withIcon(AllIcons.Nodes.Class)
                if (pkg.isNotEmpty()) b = b.withTailText("  ($pkg)", true)
                // Import only when it lives in a different, named package.
                if (pkg.isNotEmpty() && pkg != curPkg) {
                    b = b.withInsertHandler(
                        dev.jux.intellij.completion.JuxAutoImport.handler("$pkg.$name", name),
                    )
                }
                add(ranked(b, P_TYPE - 5.0), name)
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

    // ---- object members after `.` (in-file type inference) ---------------------

    /**
     * Member completion for `recv.<caret>`: resolve the receiver to an
     * in-file/project type ([dev.jux.intellij.resolve.JuxTypeInference]) and
     * offer its members — methods, fields, properties, enum constants —
     * including inherited ones (via [dev.jux.intellij.resolve.JuxHierarchy]),
     * filtered by static vs instance access. This is the IDE-side stand-in for
     * the LSP's type-aware member list; it covers user-defined project types.
     * Rust std / crate members still need the LSP (its stub index), which is
     * why the fallback never claims to be exhaustive after a dot.
     */
    private fun addMemberCompletion(parameters: CompletionParameters, result: CompletionResultSet) {
        val word = wordBeforeDot(parameters) ?: return
        val target = dev.jux.intellij.resolve.JuxTypeInference
            .resolveReceiver(word, parameters.position) ?: return
        val seen = HashSet<String>()
        for (m in dev.jux.intellij.resolve.JuxHierarchy.allMembers(target.type)) {
            val named = m as? JuxNamedElement ?: continue
            val name = named.name ?: continue
            val isEnumConst = m.elementType === E.ENUM_CONSTANT
            val isStatic = isEnumConst ||
                dev.jux.intellij.resolve.JuxHierarchy.hasModifier(m, "static")
            // Static receiver (`Type.`) → statics + enum constants; instance
            // receiver (`obj.`) → instance members only.
            if (target.isStatic != isStatic) continue
            if (!seen.add(name)) continue
            when (m.elementType) {
                E.METHOD_DECLARATION -> result.addElement(method(named, name))
                E.FIELD_DECLARATION ->
                    result.addElement(declaration(m, name, AllIcons.Nodes.Field, P_MEMBER))
                E.PROPERTY_DECLARATION ->
                    result.addElement(declaration(m, name, AllIcons.Nodes.Property, P_MEMBER))
                E.ENUM_CONSTANT ->
                    result.addElement(
                        ranked(
                            LookupElementBuilder.create(name)
                                .withIcon(AllIcons.Nodes.Enum)
                                .withTypeText(target.type.name, true),
                            P_MEMBER,
                        ),
                    )
            }
        }
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

    /**
     * The builtin annotation set, canonical casing: `override` (lowercase, the
     * spelling the missing-override quick-fix inserts), the five §TS.1 test/hook
     * names (single-sourced from [dev.jux.intellij.run.JuxTestDetector]), and the
     * C-FFI annotations (§L): `extern` (native blocks), `export` (C linkage),
     * `layout` (C-compatible structs/enums). No import is ever needed for these;
     * annotation lookups are case-insensitive compiler-side.
     */
    private fun addBuiltinAnnotations(result: CompletionResultSet) {
        val names = listOf("override", "extern", "export", "layout") +
            dev.jux.intellij.run.JuxTestDetector.TEST_HOOKS.values
        for (name in names) {
            result.addElement(
                ranked(
                    LookupElementBuilder.create(name)
                        .withIcon(AllIcons.Nodes.Annotationtype)
                        .withTypeText("builtin", true),
                    P_KEYWORD,
                ),
            )
        }
    }

    // ---- completion inside `${…}` interpolation holes --------------------------

    /**
     * The plugin OWNS completion inside string literals: returns true (so the
     * caller stops) for any caret inside a string/char token. Within an active
     * `${ … }` interpolation hole it offers the same ranked, scope-filtered
     * suggestions an expression position would get — locals/params/members/
     * types, or a receiver's members after a `.`. Everywhere else inside a
     * string (plain text, char literals) it adds NOTHING — code completion
     * there is just noise. Returns false only when the caret isn't in a string
     * at all, letting normal code completion proceed.
     *
     * The literal is a single lexer token, so the platform's default prefix is
     * unreliable here; we recompute the prefix matcher from the identifier run
     * immediately left of the caret so "whatever I type" filters correctly.
     */
    private fun addInterpHoleCompletion(
        parameters: CompletionParameters,
        result: CompletionResultSet,
    ): Boolean {
        if (!isInsideStringLiteral(parameters.position)) return false
        if (inInterpHole(parameters)) {
            val text = parameters.editor.document.charsSequence
            // ALWAYS install our own prefix matcher (even an empty one): the
            // platform's default prefix for a caret inside a string token is the
            // leading literal text (e.g. `v=${ p.`), which matches nothing, so
            // member/declaration items would be filtered out entirely.
            val res = result.withPrefixMatcher(identifierPrefix(text, parameters.offset))
            if (isAfterDot(parameters)) {
                addMemberCompletion(parameters, res)
                addPropertySurface(parameters, res)
            } else {
                addVisibleDeclarations(parameters, res)
            }
        }
        return true
    }

    /** The element (or a near ancestor) at the caret is a string/char literal. */
    private fun isInsideStringLiteral(position: PsiElement): Boolean {
        var e: PsiElement? = position
        var hops = 0
        while (e != null && hops < 3) {
            val t = e.elementType
            if (t != null &&
                (dev.jux.intellij.highlight.JuxTokenTypes.STRING_LITERALS.contains(t) ||
                    t === dev.jux.intellij.highlight.JuxTokenTypes.CHAR_LITERAL)
            ) {
                return true
            }
            e = e.parent
            hops++
        }
        return false
    }

    /**
     * True when the caret is inside an open `${ … }` hole of an interpolation
     * literal. Gate: the caret element is (within a couple of hops) an
     * interpolation string token; then a brace-aware backward scan from the
     * caret must reach an unmatched `{` whose preceding char is `$` before it
     * hits the string boundary (`"`) or a newline. A nested string inside the
     * hole short-circuits the scan (no completion there) — an accepted v1 gap.
     */
    private fun inInterpHole(parameters: CompletionParameters): Boolean {
        if (!isInsideInterpString(parameters.position)) return false
        val text = parameters.editor.document.charsSequence
        var i = parameters.offset - 1
        var depth = 0
        while (i >= 0) {
            when (text[i]) {
                '}' -> depth++
                '{' -> {
                    if (depth == 0) return i > 0 && text[i - 1] == '$'
                    depth--
                }
                '"', '\n' -> return false
            }
            i--
        }
        return false
    }

    /** The element (or a near ancestor) at the caret is an interpolation literal. */
    private fun isInsideInterpString(position: PsiElement): Boolean {
        var e: PsiElement? = position
        var hops = 0
        while (e != null && hops < 3) {
            val t = e.elementType
            if (t === dev.jux.intellij.highlight.JuxTokenTypes.INTERP_STRING_LITERAL ||
                t === dev.jux.intellij.highlight.JuxTokenTypes.INTERP_RAW_STRING_LITERAL
            ) {
                return true
            }
            e = e.parent
            hops++
        }
        return false
    }

    /** The run of identifier chars (`[A-Za-z0-9_]`) immediately left of [offset]. */
    private fun identifierPrefix(text: CharSequence, offset: Int): String {
        var i = offset
        while (i > 0 && (text[i - 1].isLetterOrDigit() || text[i - 1] == '_')) i--
        return text.subSequence(i, offset).toString()
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
     * True when the caret sits in an annotation-name position — the (possibly
     * partial) word being completed directly follows an `@` (no whitespace:
     * `@ Test` is not annotation syntax).
     */
    private fun isAfterAt(parameters: CompletionParameters): Boolean {
        val text = parameters.editor.document.charsSequence
        var i = parameters.offset - 1
        while (i >= 0 && (text[i].isLetterOrDigit() || text[i] == '_')) i--
        return i >= 0 && text[i] == '@'
    }

    /**
     * True when an LSP client (native or LSP4IJ) is serving `juxc-lsp`
     * completions for this project, so the fallback items would only duplicate
     * them. Delegates to the shared [dev.jux.intellij.lsp.JuxLspState] gate
     * (also used by the semantic annotator) — returns false in unit-test mode
     * so the fixture exercises this fallback.
     */
    private fun lspProvidesCompletion(parameters: CompletionParameters): Boolean =
        dev.jux.intellij.lsp.JuxLspState.isServing(parameters.position.project)
}
