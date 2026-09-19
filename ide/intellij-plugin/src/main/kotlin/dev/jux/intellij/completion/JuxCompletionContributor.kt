package dev.jux.intellij.completion

import com.intellij.codeInsight.completion.CompletionContributor
import com.intellij.codeInsight.completion.CompletionParameters
import com.intellij.codeInsight.completion.CompletionProvider
import com.intellij.codeInsight.completion.CompletionResultSet
import com.intellij.codeInsight.completion.CompletionType
import com.intellij.codeInsight.completion.CompletionUtil
import com.intellij.codeInsight.completion.PrefixMatcher
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
import dev.jux.intellij.highlight.JuxKeywords
import dev.jux.intellij.highlight.JuxTokenTypes as T
import dev.jux.intellij.psi.JuxElementTypes as E
import dev.jux.intellij.psi.JuxFile
import dev.jux.intellij.psi.JuxLocals
import dev.jux.intellij.psi.JuxNamedElement
import dev.jux.intellij.psi.JuxObservableProps
import dev.jux.intellij.psi.JuxPropertyDeclaration
import javax.swing.Icon

/**
 * Jux code completion, owned by the plugin whether or not `juxc-lsp` runs
 * (the LSP clients' own completion is switched off): contextual keywords, the
 * declarations *visible from the caret*, a receiver's members after `.`, name
 * suggestions for a new declaration, and smart completion
 * (`CompletionType.SMART`) of only what fits the expected type.
 *
 * Order is decided by [JuxCompletionRanking]'s sorter, as Java's is: prefix
 * quality, then the expected type at the caret, then locality, usage
 * statistics, accessibility and deprecation. The `P_*` tiers below are its
 * last resort, and describe the locality order:
 *
 *  1. locals declared before the caret + enclosing parameters (and the
 *     implicit `value` in a setter body),
 *  2. members of the enclosing class — fields, properties, methods,
 *  3. position-legal keywords ([JuxKeywordContext] — `class` is never offered
 *     inside a method body, `return` never at the top level),
 *  4. type names — this file's first, then the rest of the project's, then
 *     the toolchain's standard library and bound crates (the generated `.jux.d`
 *     stubs `JuxLibraryRootsProvider` indexes). Each of the three is a distinct
 *     tier, so `Vec` is always offered but never outranks a type the user
 *     wrote.
 *
 * Nothing else is offered: locals of OTHER methods and members of OTHER
 * classes are unreachable from the caret and would only be noise. After a
 * `.` the receiver's members come from the type engine, together with the §P
 * property surface (`.observers` + its ops, `bind`/`unbind`/
 * `bindBidirectional`).
 */
class JuxCompletionContributor : CompletionContributor() {
    internal companion object {
        // Relevance tiers (higher floats to the top of the lookup).
        const val P_LOCAL = 100.0
        const val P_PARAM = 90.0
        const val P_MEMBER = 80.0
        const val P_KEYWORD = 60.0
        const val P_TYPE = 50.0

        /**
         * A type declared in another file of the PROJECT — one the user wrote,
         * so it outranks anything from a library but not what is in front of
         * them.
         */
        const val P_TYPE_PROJECT = 45.0

        /**
         * A type from a generated `.jux.d` stub: the standard library, or a
         * bound Rust crate. Real and worth offering — that is the point of
         * indexing the toolchain's stubs — but a `Window` the user declared is
         * far likelier to be the one they mean than `minifb`'s, so the whole
         * library surface sits below the project's. Ranked last rather than
         * hidden: `Vec` and `HashMap` are typed constantly.
         */
        const val P_TYPE_LIBRARY = 40.0

        /** Cap on the backward scan in [isTypeOnlyContext] (keeps it O(1)-ish). */
        const val MAX_LOOKBACK = 240
    }

    init {
        extend(
            CompletionType.BASIC,
            PlatformPatterns.psiElement().withLanguage(JuxLanguage),
            object : CompletionProvider<CompletionParameters>() {
                override fun addCompletions(
                    parameters: CompletionParameters,
                    context: ProcessingContext,
                    rawResult: CompletionResultSet,
                ) {
                    // A comment is prose. Nothing below belongs there, and this
                    // has to come first — the interpolation-hole and property
                    // surfaces below run even under an active LSP, so a guard
                    // placed after them would still pop a list inside `// …`.
                    if (isInsideComment(parameters.position)) return

                    // Every item below is ordered by the Jux sorter (prefix
                    // quality, expected type, locality, statistics,
                    // accessibility, deprecation, then the old tiers), not by
                    // the order it happens to be added in.
                    val result = rawResult.withRelevanceSorter(
                        JuxCompletionRanking.sorter(parameters, rawResult.prefixMatcher),
                    )

                    // `Vec<String> |`, `HttpClient |`: the caret names a new
                    // declaration, so only names fit there, as in Java.
                    if (JuxNameSuggestions.isDeclarationName(parameters.position)) {
                        JuxNameSuggestions.suggest(parameters.position).forEach { result.addElement(it) }
                        result.stopHere()
                        return
                    }

                    // Inside a `$"…${ ⟨caret⟩ }…"` interpolation hole the LSP is
                    // blind — the whole literal is one opaque string token to it,
                    // so it never completes there. The plugin therefore OWNS hole
                    // completion and runs it even when the LSP is serving the rest
                    // of the file. Must come before the LSP early-return below.
                    if (addInterpHoleCompletion(parameters, result)) return

                    // The §P property surface (`.observers` + its ops
                    // attach/detach/clear/size, and `bind`/`unbind`/
                    // `bindBidirectional` on a property) is plugin-owned even
                    // under an active LSP — juxc-lsp doesn't model the observable
                    // surface, so offer it BEFORE standing down for the server
                    // (same exception as interp-hole completion above). Without
                    // this, an active LSP session would suppress observer-member
                    // completion entirely.
                    val afterDot = isAfterDot(parameters)
                    if (afterDot) addPropertySurface(parameters, result)

                    // An active LSP session supplies smarter versions of
                    // everything ELSE this contributor offers — stand down. (The
                    // probe only reports "active" when juxc-lsp can ACTUALLY
                    // serve — toolchain resolvable + session up — so a missing
                    // or broken toolchain never silences the fallback.)
                    // Hybrid engine: the plugin owns completion whether or not
                    // juxc-lsp is serving (the client's own completion is
                    // switched off in JuxLspDescriptor), so nothing stands down.

                    // Inside an `import a.b.<caret>;` path. Must be tested BEFORE
                    // the member branch: a dotted import path is also "after a
                    // dot", and routing it to member completion produced an
                    // empty popup — the receiver `b` resolves to no type, and
                    // the branch returns having offered nothing at all.
                    if (addImportPathCompletion(parameters, result)) return

                    // After `::` of a method reference: the qualifier's methods.
                    if (addMethodRefCompletion(parameters, result::addElement)) return

                    // After a `.` (member access): offer the receiver's real
                    // members (methods/fields/properties/enum constants of an
                    // in-file-resolvable type — see JuxTypeInference). The §P
                    // property surface was already added above.
                    if (afterDot) {
                        addMemberCompletion(parameters, result::addElement)
                        return
                    }

                    // Right after `case` in a switch over an enum or a sealed
                    // type: the constants and subtypes no arm names yet.
                    if (JuxCaseCompletion.addTo(parameters, result::addElement)) return

                    // After `@` — only the builtin annotations exist in Phase 1
                    // (`@override` + the §TS.1 test/hook five), so nothing else
                    // belongs in the list.
                    if (isAfterAt(parameters)) {
                        addBuiltinAnnotations(result)
                        return
                    }

                    // Type-only positions (`new X`, `extends X`, `implements X`):
                    // a type name is the ONLY grammatically legal token here, so
                    // suppress keywords and value-position declarations (locals,
                    // params, members) entirely and offer just the visible +
                    // project type names. This is the context-awareness that keeps
                    // the popup showing only relevant options.
                    if (isTypeOnlyContext(parameters)) {
                        // After `new`, the type is a constructor call: accepting
                        // `Truck` writes `Truck(` + `)` with the caret inside, as
                        // Java does, on top of any import the item adds.
                        addVisibleDeclarations(
                            parameters,
                            result::addElement,
                            typesOnly = true,
                            constructorCall = isAfterNew(parameters),
                            matcher = result.prefixMatcher,
                        )
                        return
                    }

                    // Tier 3: only the keywords the grammar accepts here.
                    val keywords = JuxKeywordContext.keywordsFor(parameters.position)
                    for (kw in keywords) {
                        result.addElement(keyword(kw))
                    }
                    // `for await (…)` (§18.6) — a two-word statement opener, so
                    // it can't ride the curated single-word sets (same reason
                    // OBSERVER joins them as a raw extra in JuxKeywordContext).
                    // Statement position is recognised by `for` (the set may
                    // be STATEMENT or STATEMENT without `yield`).
                    if ("for" in keywords) {
                        result.addElement(keyword("for await"))
                    }
                    // Where a member begins: the inherited methods this class
                    // can implement or override, written out in full on accept.
                    if (keywords === JuxKeywordContext.MEMBER) {
                        JuxOverrideCompletion.addTo(parameters, result::addElement)
                    }

                    // Tiers 1, 2, 4: declarations visible from the caret only.
                    addVisibleDeclarations(parameters, result::addElement, typesOnly = false, matcher = result.prefixMatcher)
                }
            },
        )

        // Smart completion (Ctrl+Shift+Space): only what fits the type the
        // caret wants, as Java's smart completion offers.
        extend(
            CompletionType.SMART,
            PlatformPatterns.psiElement().withLanguage(JuxLanguage),
            object : CompletionProvider<CompletionParameters>() {
                override fun addCompletions(
                    parameters: CompletionParameters,
                    context: ProcessingContext,
                    rawResult: CompletionResultSet,
                ) {
                    if (isInsideComment(parameters.position) || isInsideStringLiteral(parameters.position)) return
                    val expected = JuxCompletionRanking.expectedTypes(parameters)
                    if (expected.isEmpty()) return
                    val result = rawResult.withRelevanceSorter(
                        JuxCompletionRanking.sorter(parameters, rawResult.prefixMatcher),
                    )
                    // Values already in reach whose type fits: locals,
                    // parameters, members, and a receiver's members after `.`.
                    // Every variable walked is kept as a chain start.
                    val roots = ArrayList<LookupElement>()
                    val sink: (LookupElement) -> Unit = { item ->
                        roots.add(item)
                        val type = JuxCompletionRanking.typeOf(item)
                        if (type !is dev.jux.intellij.resolve.JuxType.Static &&
                            JuxCompletionRanking.bestFit(type, expected) == 0
                        ) {
                            result.addElement(item)
                        }
                    }
                    if (isAfterDot(parameters)) {
                        addEngineMemberCompletion(parameters, sink)
                        return
                    }
                    addVisibleDeclarations(parameters, sink, typesOnly = false, includeTypes = false)
                    JuxSmartCompletion.addTypeFitting(parameters, expected, result::addElement)
                    // Chains from those variables that reach the wanted type.
                    JuxChainCompletion.addTo(expected, roots, result::addElement)
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
    private fun addVisibleDeclarations(
        parameters: CompletionParameters,
        sink: (LookupElement) -> Unit,
        typesOnly: Boolean = false,
        constructorCall: Boolean = false,
        includeTypes: Boolean = true,
        matcher: PrefixMatcher? = null,
    ) {
        val offset = parameters.offset
        val seen = HashSet<String>()
        fun add(element: LookupElement, name: String) {
            if (seen.add(name)) sink(if (constructorCall) constructorCall(element) else element)
        }

        // Value-position tiers (locals, params, members, setter `value`) — skipped
        // in a type-only position, where none of them could legally appear.
        var from: PsiElement = parameters.position
        var scope: PsiElement? = if (typesOnly) null else parameters.position.parent
        while (scope != null && scope !is JuxFile) {
            // Pattern binders: `case Circle(var r) -> r`, `if (x => Dog d) d`.
            for (binder in JuxLocals.bindersInScope(scope, from)) {
                val named = binder as? JuxNamedElement ?: continue
                val name = named.name ?: continue
                add(declaration(named, name, AllIcons.Nodes.Variable, P_LOCAL), name)
            }
            when (scope.elementType) {
                E.CODE_BLOCK ->
                    // Locals are visible only after their declaration, a
                    // destructuring's binders included. The sorter puts the
                    // nearest one first.
                    for (child in JuxLocals.blockLocals(scope)) {
                        if (child.elementType !== E.LOCAL_VARIABLE) continue
                        if (child.textOffset >= offset) continue
                        val named = child as? JuxNamedElement ?: continue
                        val name = named.name ?: continue
                        add(declaration(named, name, AllIcons.Nodes.Variable, P_LOCAL), name)
                    }
                // A loop's own binding, a caught exception, a lambda's
                // parameters. Each introduces a name for the length of its
                // construct, and each used to be invisible because the parser
                // consumed it as raw tokens.
                E.FOR_EACH_STATEMENT, E.FOR_STATEMENT, E.CATCH_CLAUSE ->
                    for (child in scope.children) {
                        if (child.elementType !== E.LOCAL_VARIABLE) continue
                        val name = (child as? JuxNamedElement)?.name ?: continue
                        add(declaration(child, name, AllIcons.Nodes.Variable, P_LOCAL), name)
                    }
                E.LAMBDA_EXPRESSION ->
                    for (p in lambdaParameters(scope)) {
                        val name = (p as? JuxNamedElement)?.name ?: continue
                        add(declaration(p, name, AllIcons.Nodes.Parameter, P_PARAM), name)
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
                E.CLASS_BODY -> {
                    // The whole member surface, INHERITED included: `shared` and
                    // `this.shared` are the same reference, and only the second
                    // used to complete — the walk read this body's own children
                    // and stopped. Visibility still applies; a `private` member
                    // of an ancestor is not in scope here. The class's own
                    // members rank above what it inherits.
                    val owner = scope.parent
                    for (m in enclosingMembers(scope, from = owner)) {
                        val named = m as? JuxNamedElement ?: continue
                        val name = named.name ?: continue
                        val kind = memberKind(m, owner)
                        when (m.elementType) {
                            E.FIELD_DECLARATION, E.CONST_DECLARATION ->
                                add(declaration(named, name, AllIcons.Nodes.Field, P_MEMBER, kind), name)
                            E.PROPERTY_DECLARATION ->
                                add(declaration(named, name, AllIcons.Nodes.Property, P_MEMBER, kind), name)
                            E.METHOD_DECLARATION ->
                                add(method(named, name, kind = kind), name)
                            else -> {}
                        }
                    }
                }
                else -> {}
            }
            from = scope
            scope = scope.parent
        }

        // Type parameters of every enclosing declaration — `T` is a legal type
        // inside `class Box<T>`, and inside a generic method's body. Offered
        // even in a type-only position, which is where they are most used.
        if (includeTypes) {
            var typeScope: PsiElement? = parameters.position.parent
            while (typeScope != null && typeScope !is JuxFile) {
                for (tp in typeParameters(typeScope)) {
                    val name = (tp as? JuxNamedElement)?.name ?: continue
                    add(declaration(tp, name, AllIcons.Nodes.Class, P_TYPE), name)
                }
                typeScope = typeScope.parent
            }
        }

        // Tier 4: file-level declarations — the file's free functions, and its
        // type names (`Model m = new Model();`), no import needed.
        val file = parameters.originalFile
        // A native fn is only callable in an unsafe context (E0506 elsewhere):
        // an `unsafe { }` block, or the body of a function declared `unsafe`
        // (§L.5.1). Only offer them there — otherwise they'd be wrong-scope
        // noise ranked above the user's own types.
        val insideUnsafe = run {
            var p: PsiElement? = parameters.position
            while (p != null && p !is JuxFile) {
                if (p.elementType === E.UNSAFE_STATEMENT) return@run true
                if (p.elementType === E.METHOD_DECLARATION &&
                    p.node.findChildByType(E.MODIFIER_LIST)?.findChildByType(T.UNSAFE_KW) != null
                ) {
                    return@run true
                }
                p = p.parent
            }
            false
        }
        for (decl in file.children) {
            // §L.7 C-FFI: surface a `native { … }` block's foreign functions as
            // file-level callables, but only inside `unsafe` (see above).
            if (decl.elementType === E.EXTERN_BLOCK) {
                if (insideUnsafe && !typesOnly) {
                    for (fn in decl.children) {
                        if (fn.elementType !== E.METHOD_DECLARATION) continue
                        val named = fn as? JuxNamedElement ?: continue
                        val name = named.name ?: continue
                        add(method(named, name, kind = JuxCompletionRanking.Kind.TOP_LEVEL), name)
                    }
                }
                continue
            }
            val named = decl as? JuxNamedElement ?: continue
            val name = named.name ?: continue
            if (decl.elementType === E.METHOD_DECLARATION && !typesOnly) {
                // A free function of this file, `main` excluded: nothing calls
                // the entry point.
                if (name != "main") add(method(named, name, kind = JuxCompletionRanking.Kind.TOP_LEVEL), name)
            } else if (includeTypes && decl.elementType in TYPE_DECLS) {
                add(declaration(named, name, AllIcons.Nodes.Class, P_TYPE), name)
            }
        }
        if (!includeTypes) return

        // Tier 4b/4c: types from OTHER files — the project's own, then its
        // dependencies' and the toolchain's. Auto-import on accept. This is
        // what lets cross-file and library types show up without the LSP;
        // the sorter keeps in-file names on top, then the user's other files,
        // then the dependency packages and the generated `.jux.d` stubs
        // (`JuxLibraryRootsProvider` puts them all into `allScope`).
        val project = parameters.position.project
        // The project type index reads FileTypeIndex, which throws
        // IndexNotReadyException during indexing (dumb mode). Completion can fire
        // then (e.g. right after open / a big VCS update), so skip the cross-file
        // walk rather than abort the whole popup; in-file names above still show.
        if (com.intellij.openapi.project.DumbService.isDumb(project)) return
        val curPkg = dev.jux.intellij.completion.JuxAutoImport.packageOfFile(file)
        // Only the files declaring a name that matches what was typed are read
        // (the name index narrows them), so the popup never walks the project.
        dev.jux.intellij.resolve.JuxTypeIndex.forEachTypeMatching(
            project,
            com.intellij.psi.search.GlobalSearchScope.allScope(project),
            { matcher == null || matcher.prefixMatches(it) },
        ) { type ->
            val name = type.name
            if (name != null && name !in seen) {
                val pkg = dev.jux.intellij.completion.JuxAutoImport.packageOf(type)
                // §4.4: a no-modifier declaration is "visible within this
                // package only". Offering one across a package boundary put a
                // name in the popup that the accepting file has no business
                // writing — and wrote an `import` for it. A modifier the editor
                // ignores is a comment.
                if (!dev.jux.intellij.resolve.JuxHierarchy.typeVisibleFrom(type, curPkg)) {
                    return@forEachTypeMatching
                }
                var b = LookupElementBuilder.create(type, name).withIcon(AllIcons.Nodes.Class)
                if (pkg.isNotEmpty()) b = b.withTailText("  ($pkg)", true)
                // Import only when it lives in a different, named package.
                if (pkg.isNotEmpty() && pkg != curPkg) {
                    b = b.withInsertHandler(
                        dev.jux.intellij.completion.JuxAutoImport.handler("$pkg.$name", name),
                    )
                }
                // A stub declares a foreign API and a dependency is someone
                // else's code: both rank below the user's own types.
                val fromLibrary = JuxCompletionRanking.isLibrary(type)
                add(ranked(b, if (fromLibrary) P_TYPE_LIBRARY else P_TYPE_PROJECT), name)
            }
        }
    }

    /**
     * How near a member of the class the caret is in sits: declared by that
     * class itself, or inherited from a supertype.
     */
    private fun memberKind(member: PsiElement, owner: PsiElement?): JuxCompletionRanking.Kind {
        val declaredIn = PsiTreeUtil.getParentOfType(member, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)
        return if (owner == null || declaredIn == null || declaredIn == owner) JuxCompletionRanking.Kind.MEMBER
        else JuxCompletionRanking.Kind.INHERITED
    }

    /**
     * A non-method declaration lookup: icon, declared-type hint, and the
     * declaration itself as the lookup object, so the sorter can read its type
     * and where it lives.
     */
    private fun declaration(
        decl: PsiElement,
        name: String,
        icon: Icon,
        priority: Double,
        kind: JuxCompletionRanking.Kind? = null,
    ): LookupElement {
        // For an auto-property the type hint reflects the EFFECTIVE type — an
        // uninitialized auto-property is implicitly nullable (§M.7.3.1), so the
        // popup shows `int?`, not `int`. Other declarations keep their declared type.
        val typeText = (decl as? JuxPropertyDeclaration)?.effectiveTypeText()
            ?: decl.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()
            ?: inferredTypeText(decl)
        var builder = LookupElementBuilder.create(CompletionUtil.getOriginalOrSelf(decl), name).withIcon(icon)
        if (typeText != null) builder = builder.withTypeText(typeText, true)
        if (JuxCompletionRanking.isDeprecated(decl)) builder = builder.strikeout()
        return ranked(builder, priority, kind)
    }

    /** A `var` local's inferred type, for the type column, when the engine knows it. */
    private fun inferredTypeText(decl: PsiElement): String? {
        if (decl.elementType !== E.LOCAL_VARIABLE) return null
        val t = dev.jux.intellij.resolve.JuxTypeEngine.declaredType(decl)
        return t.takeUnless { it is dev.jux.intellij.resolve.JuxType.Unknown }?.presentable()
    }

    /**
     * A method lookup, shaped the way Java shows one: the return type in the
     * type column, the parameter list as tail text, and on accept `()` with
     * the caret inside when it takes arguments, after it when it takes none.
     */
    private fun method(
        decl: JuxNamedElement,
        name: String,
        presentedReturnType: String? = null,
        kind: JuxCompletionRanking.Kind? = null,
        returnType: dev.jux.intellij.resolve.JuxType? = null,
    ): LookupElement {
        val params = (decl as PsiElement).node.findChildByType(E.PARAMETER_LIST)?.text ?: "()"
        val typeText = presentedReturnType ?: decl.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()
        var builder = LookupElementBuilder.create(CompletionUtil.getOriginalOrSelf(decl as PsiElement), name)
            .withIcon(AllIcons.Nodes.Method)
            .withTailText(params.replace(Regex("\\s+"), " "), true)
            .withInsertHandler(ParenthesesInsertHandler.getInstance(dev.jux.intellij.resolve.JuxHierarchy.arity(decl) > 0))
        if (typeText != null) builder = builder.withTypeText(typeText, true)
        if (JuxCompletionRanking.isDeprecated(decl)) builder = builder.strikeout()
        val element = ranked(builder, P_MEMBER, kind)
        if (returnType != null) element.putUserData(JuxCompletionRanking.TYPE, returnType)
        return element
    }

    /** A keyword: bold, ranked by the sorter below the names that match as well. */
    private fun keyword(word: String): LookupElement =
        ranked(LookupElementBuilder.create(word).bold(), P_KEYWORD)

    /**
     * Every member reachable unqualified from inside a class body: the class's
     * own, plus everything it inherits that is visible from here.
     *
     * [dev.jux.intellij.resolve.JuxHierarchy.allMembers] already walks the
     * supertype chain; the only thing this adds is the visibility gate and a
     * graceful answer when the body has no enclosing declaration (a malformed
     * file mid-edit), where it falls back to the body's own children.
     */
    private fun enclosingMembers(classBody: PsiElement, from: PsiElement?): List<PsiElement> {
        val decl = from as? dev.jux.intellij.psi.JuxTypeDeclaration
            ?: return classBody.children.toList()
        return dev.jux.intellij.resolve.JuxHierarchy.allMembers(decl)
            .filter { dev.jux.intellij.resolve.JuxHierarchy.memberVisibleFrom(it, decl) }
    }

    /** A lambda's declared parameters, or empty for the bare `x -> …` form. */
    private fun lambdaParameters(lambda: PsiElement): List<PsiElement> {
        val list = lambda.children.firstOrNull { it.elementType === E.PARAMETER_LIST }
        // `x -> …` puts the single PARAMETER directly under the lambda.
        val direct = lambda.children.filter { it.elementType === E.PARAMETER }
        return (list?.children?.filter { it.elementType === E.PARAMETER } ?: emptyList()) + direct
    }

    /** The type parameters [scope] declares, if it declares any. */
    private fun typeParameters(scope: PsiElement): List<PsiElement> =
        scope.node.findChildByType(E.TYPE_PARAMETER_LIST)?.psi
            ?.children?.filter { it.elementType === E.TYPE_PARAMETER }
            ?: emptyList()

    // ---- import paths ----------------------------------------------------------

    /**
     * Completion inside an `import a.b.<caret>;` path: the next package segment,
     * then the types that package declares. Reports whether it handled the caret.
     *
     * The prefix already typed is matched against every package in the project,
     * so `import demo.` offers `model` (the next segment of `demo.model.core`)
     * and `import demo.model.` offers that package's types. Types insert bare —
     * the path is already written, and no `import` line is needed for an import.
     *
     * Package names come from the declaring files' `package …;` statements
     * ([JuxAutoImport.packageOf]), which is the same source auto-import uses, so
     * the two cannot disagree about where a type lives.
     */
    private fun addImportPathCompletion(
        parameters: CompletionParameters,
        result: CompletionResultSet,
    ): Boolean {
        val prefix = importPathPrefix(parameters) ?: return false
        val project = parameters.position.project
        if (com.intellij.openapi.project.DumbService.isDumb(project)) return true
        val segments = LinkedHashSet<String>()
        val types = ArrayList<Pair<String, dev.jux.intellij.psi.JuxTypeDeclaration>>()
        dev.jux.intellij.resolve.JuxTypeIndex.forEachType(
            project,
            com.intellij.psi.search.GlobalSearchScope.allScope(project),
        ) { type ->
            val pkg = JuxAutoImport.packageOf(type)
            val name = type.name
            when {
                // A type declared exactly in the typed package.
                pkg == prefix && name != null -> types.add(name to type)
                // A deeper package: offer only its next segment.
                pkg.startsWith("$prefix.") ->
                    segments.add(pkg.removePrefix("$prefix.").substringBefore('.'))
            }
        }
        for (seg in segments) {
            result.addElement(
                ranked(
                    LookupElementBuilder.create(seg).withIcon(AllIcons.Nodes.Package),
                    P_TYPE,
                ),
            )
        }
        for ((name, type) in types) {
            val fromStub = type.containingFile?.name?.endsWith(".jux.d") == true
            result.addElement(
                ranked(
                    LookupElementBuilder.create(name).withIcon(AllIcons.Nodes.Class),
                    if (fromStub) P_TYPE_LIBRARY else P_TYPE_PROJECT,
                ),
            )
        }
        return true
    }

    /**
     * The already-typed package prefix when the caret sits in an import path —
     * `import demo.model.<caret>` → `"demo.model"`. `null` when the caret is not
     * in an `import` statement, so the caller falls through to normal
     * completion.
     *
     * Read off the document rather than the PSI: mid-edit the trailing dot
     * leaves the import statement unparsed, so there is no reliable
     * `IMPORT_STATEMENT` node to walk at exactly the moment completion fires.
     */
    private fun importPathPrefix(parameters: CompletionParameters): String? {
        val text = parameters.originalFile.viewProvider.document?.charsSequence ?: return null
        val caret = parameters.offset
        // Back to the start of the line.
        var lineStart = caret
        while (lineStart > 0 && text[lineStart - 1] != '\n') lineStart--
        val line = text.subSequence(lineStart, caret).toString().trimStart()
        if (!line.startsWith("import ")) return null
        val path = line.removePrefix("import ").trimStart()
        // Only a path-shaped run qualifies; a grouped or wildcard import is a
        // different grammar and is left alone.
        if (path.any { !(it.isLetterOrDigit() || it == '_' || it == '.') }) return null
        val prefix = path.substringBeforeLast('.', "")
        return prefix.ifEmpty { null }
    }

    // ---- object members after `.` (in-file type inference) ---------------------

    /**
     * Member completion for `recv.<caret>`: resolve the receiver to an
     * in-file/project type ([dev.jux.intellij.resolve.JuxTypeInference]) and
     * offer its members — methods, fields, properties, enum constants —
     * including inherited ones (via [dev.jux.intellij.resolve.JuxHierarchy]),
     * filtered by static vs instance access.
     *
     * This covers the standard library and bound crates too: their generated
     * `.jux.d` stubs are indexed as an external library
     * ([dev.jux.intellij.resolve.JuxLibraryRootsProvider]), so `Vec` and
     * `minifb::Window` resolve like any other declaration and their real
     * members are what gets offered. The LSP still gives a better list when it
     * is running — it knows inferred types, not just declared ones — which is
     * why this stands down entirely while a server is attached.
     */
    private fun addMemberCompletion(parameters: CompletionParameters, sink: (LookupElement) -> Unit) {
        if (addEngineMemberCompletion(parameters, sink)) return
        val expression = receiverExpressionBeforeDot(parameters) ?: return
        val target = dev.jux.intellij.resolve.JuxTypeInference
            .resolveReceiverExpression(expression, parameters.position) ?: return
        val seen = HashSet<String>()
        // Where the caret is decides what it may reach: another class's
        // `private` members are names that do not compile, and offering them
        // taught the popup to be wrong.
        val from = PsiTreeUtil.getParentOfType(
            parameters.position,
            dev.jux.intellij.psi.JuxTypeDeclaration::class.java,
        )
        for (m in dev.jux.intellij.resolve.JuxHierarchy.allMembers(target.type)) {
            val named = m as? JuxNamedElement ?: continue
            val name = named.name ?: continue
            if (!dev.jux.intellij.resolve.JuxHierarchy.memberVisibleFrom(m, from)) continue
            val isEnumConst = m.elementType === E.ENUM_CONSTANT
            val isStatic = isEnumConst ||
                dev.jux.intellij.resolve.JuxHierarchy.hasModifier(m, "static")
            // Static receiver (`Type.`) → statics + enum constants; instance
            // receiver (`obj.`) → instance members only.
            if (target.isStatic != isStatic) continue
            if (!seen.add(name)) continue
            val kind = memberKind(m, target.type)
            when (m.elementType) {
                E.METHOD_DECLARATION -> sink(method(named, name, kind = kind))
                E.FIELD_DECLARATION, E.CONST_DECLARATION ->
                    sink(declaration(m, name, AllIcons.Nodes.Field, P_MEMBER, kind))
                E.PROPERTY_DECLARATION, E.RECORD_COMPONENT ->
                    sink(declaration(m, name, AllIcons.Nodes.Property, P_MEMBER, kind))
                E.ENUM_CONSTANT ->
                    sink(
                        ranked(
                            LookupElementBuilder.create(CompletionUtil.getOriginalOrSelf(m), name)
                                .withIcon(AllIcons.Nodes.Enum)
                                .withTypeText(target.type.name, true),
                            P_MEMBER,
                            kind,
                        ),
                    )
            }
        }
    }

    /**
     * Members after `qualifier.` from the type engine: the qualifier's real
     * type, followed through calls, chains, `var` inference, generics and a
     * type parameter's bound, with each member's type shown as the receiver
     * sees it (`Box<Truck>.get()` reads `Truck`, not `T`). Reports whether the
     * qualifier had a type at all; when it did not, the caller falls back.
     *
     * Project types and the `.jux.d` stubs of the standard library, bound
     * crates and dependency packages all come through here alike. After a
     * type name only its statics are offered (an enum's variants among them),
     * after a value only its instance members plus the two named operators
     * every value has (`operator hash()`, `operator string()`). The receiver
     * type's own members rank above what it inherits, and every item carries
     * its type so the sorter can put what fits the caret first.
     */
    private fun addEngineMemberCompletion(parameters: CompletionParameters, sink: (LookupElement) -> Unit): Boolean {
        val access = parameters.position.parent ?: return false
        if (access.elementType !== E.FIELD_ACCESS_EXPRESSION) return false
        val qualifier = dev.jux.intellij.resolve.JuxTypeEngine.firstExpressionChild(access) ?: return false
        val qualifierType = dev.jux.intellij.resolve.JuxTypeEngine.typeOf(qualifier)
        if (qualifierType is dev.jux.intellij.resolve.JuxType.Unknown) return false
        // A tuple's elements, `.0`, `.1`, each with its type.
        val tuple = dev.jux.intellij.resolve.JuxTypeEngine.stripNullable(qualifierType) as?
            dev.jux.intellij.resolve.JuxType.TupleType
        if (tuple != null) {
            tuple.elements.forEachIndexed { i, t ->
                sink(LookupElementBuilder.create(i.toString()).withTypeText(t.presentable()))
            }
            return true
        }
        val static = dev.jux.intellij.resolve.JuxTypeEngine.stripNullable(qualifierType) is
            dev.jux.intellij.resolve.JuxType.Static
        if (!static) addNamedOperators(qualifierType, sink)
        val receiverClass = dev.jux.intellij.resolve.JuxTypeEngine.classOf(qualifierType) ?: return true
        val from = PsiTreeUtil.getParentOfType(parameters.position, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)
        val seen = HashSet<String>()
        // A second Ctrl+Space also lists what the caret cannot reach, as Java
        // does, marked so the sorter sinks it below everything reachable.
        val showInaccessible = parameters.invocationCount >= 2
        for (member in dev.jux.intellij.resolve.JuxTypeEngine.membersOf(qualifierType)) {
            val m = member.element
            val named = m as? JuxNamedElement ?: continue
            val name = named.name ?: continue
            val visible = dev.jux.intellij.resolve.JuxHierarchy.memberVisibleFrom(m, from)
            if (!visible && !showInaccessible) continue
            val emit: (LookupElement) -> Unit = if (visible) sink else { item ->
                item.putUserData(JuxCompletionRanking.INACCESSIBLE, true)
                sink(item)
            }
            if (dev.jux.intellij.resolve.JuxTypeEngine.isStaticMember(m) != static) continue
            val key = if (m.elementType === E.METHOD_DECLARATION) {
                "$name/${dev.jux.intellij.resolve.JuxHierarchy.arity(m)}"
            } else {
                name
            }
            if (!seen.add(key)) continue
            val kind = if (member.owner.decl == receiverClass.decl) JuxCompletionRanking.Kind.MEMBER
            else JuxCompletionRanking.Kind.INHERITED
            when (m.elementType) {
                E.METHOD_DECLARATION -> {
                    val returnType = dev.jux.intellij.resolve.JuxTypeEngine.returnType(member)
                    val known = returnType.takeUnless { it is dev.jux.intellij.resolve.JuxType.Unknown }
                    emit(method(named, name, known?.presentable(), kind, known))
                }
                E.FIELD_DECLARATION, E.CONST_DECLARATION, E.RECORD_COMPONENT, E.PROPERTY_DECLARATION -> {
                    val type = dev.jux.intellij.resolve.JuxTypeEngine.memberType(member)
                    // A record's components read as properties (`p.x`), so
                    // they show as properties, as a declared property does.
                    val icon = if (m.elementType === E.PROPERTY_DECLARATION || m.elementType === E.RECORD_COMPONENT) {
                        AllIcons.Nodes.Property
                    } else {
                        AllIcons.Nodes.Field
                    }
                    var b = LookupElementBuilder.create(CompletionUtil.getOriginalOrSelf(m), name).withIcon(icon)
                    val text = type.takeUnless { it is dev.jux.intellij.resolve.JuxType.Unknown }?.presentable()
                        ?: m.node.findChildByType(E.TYPE_REFERENCE)?.text?.trim()
                    if (text != null) b = b.withTypeText(text, true)
                    if (JuxCompletionRanking.isDeprecated(m)) b = b.strikeout()
                    val element = ranked(b, P_MEMBER, kind)
                    if (type !is dev.jux.intellij.resolve.JuxType.Unknown) element.putUserData(JuxCompletionRanking.TYPE, type)
                    emit(element)
                }
                E.ENUM_CONSTANT -> emit(
                    ranked(
                        LookupElementBuilder.create(CompletionUtil.getOriginalOrSelf(m), name)
                            .withIcon(AllIcons.Nodes.Enum)
                            .withTypeText(member.owner.decl.name, true),
                        P_MEMBER,
                        kind,
                    ),
                )
            }
        }
        // An enum's built-in helpers (§7.7.3): `Color.fromName(..)`,
        // `c.ordinal()`. Nothing declares them, so they come from the table.
        for (builtin in dev.jux.intellij.resolve.JuxEnumBuiltins.of(receiverClass.decl, static)) {
            if (!seen.add("${builtin.name}/builtin")) continue
            val type = dev.jux.intellij.resolve.JuxEnumBuiltins.returnType(receiverClass.decl, builtin, parameters.position)
            val shown = type.takeUnless { it is dev.jux.intellij.resolve.JuxType.Unknown }?.presentable()
                ?: builtin.returns.replace("Self", receiverClass.decl.name ?: "Self")
            val takesArgs = builtin.params != "()"
            val item = LookupElementBuilder.create(builtin.name)
                .withIcon(AllIcons.Nodes.Method)
                .withTailText(builtin.params, true)
                .withTypeText(shown, true)
                .withInsertHandler { ctx, _ ->
                    // `name(` + `)`, the caret inside when arguments are due.
                    val doc = ctx.document
                    doc.insertString(ctx.tailOffset, "()")
                    ctx.editor.caretModel.moveToOffset(ctx.tailOffset - if (takesArgs) 1 else 0)
                }
            val element = ranked(item, P_MEMBER, JuxCompletionRanking.Kind.INHERITED)
            if (type !is dev.jux.intellij.resolve.JuxType.Unknown) element.putUserData(JuxCompletionRanking.TYPE, type)
            sink(element)
        }
        return true
    }

    /**
     * `obj::` / `Type::` (§M.8): the methods a method reference can name.
     * After a value, its instance methods (`g::greet`, bound); after a type,
     * its static methods, its instance methods (unbound, the receiver becomes
     * the first argument) and `new` for its constructor. A reference is not a
     * call, so accepting one writes the bare name, as Java does. Reports
     * whether the caret was after `::` at all.
     */
    private fun addMethodRefCompletion(parameters: CompletionParameters, sink: (LookupElement) -> Unit): Boolean {
        val ref = parameters.position.parent ?: return false
        if (ref.elementType !== E.METHOD_REF_EXPRESSION) return false
        var prev = parameters.position.prevSibling
        while (prev is com.intellij.psi.PsiWhiteSpace) prev = prev.prevSibling
        if (prev?.elementType !== T.COLON_COLON) return false
        val engine = dev.jux.intellij.resolve.JuxTypeEngine
        val qualifier = engine.firstExpressionChild(ref) ?: ref.node.findChildByType(E.TYPE_REFERENCE)?.psi ?: return true
        val qualifierType = if (qualifier.elementType === E.TYPE_REFERENCE) {
            engine.typeOfTypeReference(qualifier).let { t -> engine.classOf(t)?.let { dev.jux.intellij.resolve.JuxType.Static(it.decl) } ?: t }
        } else {
            engine.typeOf(qualifier)
        }
        val onType = engine.stripNullable(qualifierType) is dev.jux.intellij.resolve.JuxType.Static
        val receiver = engine.classOf(qualifierType) ?: return true
        val from = PsiTreeUtil.getParentOfType(parameters.position, dev.jux.intellij.psi.JuxTypeDeclaration::class.java)
        val seen = HashSet<String>()
        // What a caller would see: instance members through a value, and
        // everything but the fields through a type.
        val members = engine.membersOf(if (onType) dev.jux.intellij.resolve.JuxType.ClassType(receiver.decl, receiver.args) else qualifierType)
        for (member in members) {
            val m = member.element
            if (m.elementType !== E.METHOD_DECLARATION) continue
            if (!onType && engine.isStaticMember(m)) continue
            if (!dev.jux.intellij.resolve.JuxHierarchy.memberVisibleFrom(m, from)) continue
            val named = m as? JuxNamedElement ?: continue
            val name = named.name ?: continue
            if (!seen.add(name)) continue
            val returnType = engine.returnType(member).takeUnless { it is dev.jux.intellij.resolve.JuxType.Unknown }
            var b = LookupElementBuilder.create(CompletionUtil.getOriginalOrSelf(m), name).withIcon(AllIcons.Nodes.Method)
            val params = m.node.findChildByType(E.PARAMETER_LIST)?.text
            if (params != null) b = b.withTailText(params, true)
            if (returnType != null) b = b.withTypeText(returnType.presentable(), true)
            val kind = if (member.owner.decl == receiver.decl) JuxCompletionRanking.Kind.MEMBER else JuxCompletionRanking.Kind.INHERITED
            sink(ranked(b, P_MEMBER, kind))
        }
        if (onType) {
            sink(ranked(LookupElementBuilder.create("new").bold().withTypeText(receiver.decl.name ?: "", true), P_MEMBER))
        }
        return true
    }

    /**
     * `x.operator string()` and `x.operator hash()`: the two named operators
     * every value can be asked for (Operators addendum, "Invoking a named
     * operator"). A type's own `operator` declaration or the built-in one
     * answers; either way the call is spelled the same.
     *
     * `operator hash` is left out where the language has none (E0933): an
     * array, and a generic library container such as `Vec<T>` or
     * `HashMap<K, V>`, whose contents can change under a shared handle. They
     * rank below the type's own members, as Java ranks `Object`'s.
     */
    private fun addNamedOperators(receiver: dev.jux.intellij.resolve.JuxType, sink: (LookupElement) -> Unit) {
        val bare = dev.jux.intellij.resolve.JuxTypeEngine.stripNullable(receiver)
        val container = bare is dev.jux.intellij.resolve.JuxType.ArrayType ||
            (bare is dev.jux.intellij.resolve.JuxType.ClassType && bare.args.isNotEmpty() &&
                JuxCompletionRanking.isLibrary(bare.decl))
        val ops = if (container) listOf("string" to "String") else listOf("string" to "String", "hash" to "int")
        for ((op, result) in ops) {
            val builder = LookupElementBuilder.create("operator $op")
                .withLookupStrings(listOf("operator $op", op))
                .withPresentableText("operator $op")
                .withIcon(AllIcons.Nodes.Method)
                .withTailText("()", true)
                .withTypeText(result, true)
                .withInsertHandler(ParenthesesInsertHandler.getInstance(false))
            val element = ranked(builder, P_MEMBER, JuxCompletionRanking.Kind.UNIVERSAL)
            element.putUserData(JuxCompletionRanking.TYPE, dev.jux.intellij.resolve.JuxType.Primitive(result))
            sink(element)
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
        // Generated from the compiler's honored set — the same list juxc-lsp
        // offers, so `@` means the same thing whether or not a server is up
        // (`@align(N)`, Layout-ABI §L.1.4, is in that list now).
        for (name in JuxKeywords.ANNOTATIONS) {
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
                addMemberCompletion(parameters, res::addElement)
                addPropertySurface(parameters, res)
            } else {
                addVisibleDeclarations(parameters, res::addElement, matcher = res.prefixMatcher)
            }
        }
        return true
    }

    /**
     * The caret sits in a comment — line, block, or doc.
     *
     * Completion there offered the full keyword and declaration list, so typing
     * a note produced a popup over prose. The LSP suppresses this correctly
     * (`build_completions` returns an empty list for any non-code scan mode),
     * and the fallback has to agree or the behaviour changes with the server.
     */
    private fun isInsideComment(position: PsiElement): Boolean {
        var e: PsiElement? = position
        var hops = 0
        while (e != null && hops < 3) {
            val t = e.elementType
            if (t != null && dev.jux.intellij.highlight.JuxTokenTypes.COMMENTS.contains(t)) {
                return true
            }
            e = e.parent
            hops++
        }
        return false
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

    /**
     * The whole receiver EXPRESSION before the `.` the caret follows —
     * `n.make()!!.leaf`, `xs[0]`, `(n.leaf)` — or null when there is no
     * receiver there.
     *
     * Scans backwards tracking bracket depth, so a chain, an index read or a
     * parenthesized expression comes back whole. [wordBeforeDot] takes only the
     * last identifier, which is why every one of those shapes used to resolve
     * to nothing.
     */
    private fun receiverExpressionBeforeDot(parameters: CompletionParameters): String? {
        val text = parameters.editor.document.charsSequence
        var i = parameters.offset - 1
        while (i >= 0 && (text[i].isLetterOrDigit() || text[i] == '_')) i--
        while (i >= 0 && text[i].isWhitespace()) i--
        if (i < 0 || text[i] != '.') return null
        // `?.` — the dot is part of a safe call; keep the `?` in the expression
        // so the resolver sees the nullable step.
        val end = i
        var j = i - 1
        var depth = 0
        while (j >= 0) {
            val c = text[j]
            when {
                c == ')' || c == ']' -> { depth++; j-- }
                c == '(' || c == '[' -> {
                    if (depth == 0) break
                    depth--
                    j--
                }
                depth > 0 -> j--
                c.isLetterOrDigit() || c == '_' || c == '.' || c == '!' || c == '?' -> j--
                else -> break
            }
        }
        val expr = text.subSequence(j + 1, end).toString().trim()
        return expr.ifEmpty { null }
    }

    /** The identifier word immediately before the `.` the caret follows, or null. */
    @Suppress("unused")
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

    /**
     * Tag an item with its tier and kind for [JuxCompletionRanking]'s sorter.
     *
     * The tier used to be a [com.intellij.codeInsight.completion.PrioritizedLookupElement]
     * priority, but the platform weighs that FIRST, above prefix quality and
     * everything else, so no smarter rule could ever reorder the list. It is
     * now the sorter's last resort. The kind defaults from the tier.
     */
    private fun ranked(
        builder: LookupElementBuilder,
        priority: Double,
        kind: JuxCompletionRanking.Kind? = null,
    ): LookupElement {
        builder.putUserData(JuxCompletionRanking.TIER, priority)
        builder.putUserData(JuxCompletionRanking.KIND, kind ?: kindForTier(priority))
        return builder
    }

    private fun kindForTier(priority: Double): JuxCompletionRanking.Kind = when (priority) {
        P_LOCAL -> JuxCompletionRanking.Kind.LOCAL
        P_PARAM -> JuxCompletionRanking.Kind.PARAM
        P_MEMBER -> JuxCompletionRanking.Kind.MEMBER
        P_KEYWORD -> JuxCompletionRanking.Kind.KEYWORD
        P_TYPE -> JuxCompletionRanking.Kind.TYPE_FILE
        P_TYPE_PROJECT -> JuxCompletionRanking.Kind.TYPE_PROJECT
        P_TYPE_LIBRARY -> JuxCompletionRanking.Kind.TYPE_LIBRARY
        else -> JuxCompletionRanking.Kind.OTHER
    }

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

    /** Keywords that make the following token a TYPE name (and only a type). */
    private val TYPE_GOVERNING = setOf("new", "extends", "implements")

    /**
     * True when the caret sits where only a TYPE name is legal — directly after
     * `new`, or within an `extends` / `implements` clause (including later
     * entries in a comma-separated `implements A, B` list). Scans backward over
     * the "type-list gap" — identifiers, dots, commas, generics punctuation,
     * whitespace — and reports true if a [TYPE_GOVERNING] keyword is reached
     * before any statement punctuation. Hitting `{ ( ; = }` etc. (which can't
     * precede a bare type name) stops the scan with false, so ordinary
     * expression positions keep their full completion set.
     */
    private fun isTypeOnlyContext(parameters: CompletionParameters): Boolean {
        val text = parameters.editor.document.charsSequence
        var i = parameters.offset - 1
        // Skip the partial identifier currently being completed.
        while (i >= 0 && (text[i].isLetterOrDigit() || text[i] == '_')) i--
        var steps = 0
        while (i >= 0 && steps < MAX_LOOKBACK) {
            val c = text[i]
            when {
                // A line break ends the scan UNLESS it's a comma-list continuation
                // (`implements A,⏎  B`); otherwise a governing keyword on an earlier
                // line — a half-typed `class C extends Base⏎ Field‸` — would wrongly
                // suppress keyword/value completion while the user types a member.
                c == '\n' -> {
                    var k = i - 1
                    while (k >= 0 && text[k] != '\n' && text[k].isWhitespace()) k--
                    if (k < 0 || text[k] != ',') return false
                    i--
                }
                // `.`/`,`/`<`/`>` are the only non-space gap chars (qualified names,
                // type lists, generics). `?` and `&` are deliberately NOT here: they
                // are ternary / bitwise / wildcard operators far more often than type
                // punctuation in the positions this scan reaches.
                c.isWhitespace() || c == '.' || c == ',' || c == '<' || c == '>' -> i--
                c.isLetterOrDigit() || c == '_' -> {
                    var j = i
                    while (j >= 0 && (text[j].isLetterOrDigit() || text[j] == '_')) j--
                    if (text.subSequence(j + 1, i + 1).toString() in TYPE_GOVERNING) return true
                    i = j
                }
                else -> return false
            }
            steps++
        }
        return false
    }

    /** True when the word before the type being completed is `new`. */
    private fun isAfterNew(parameters: CompletionParameters): Boolean {
        val text = parameters.editor.document.charsSequence
        var i = parameters.offset - 1
        while (i >= 0 && (text[i].isLetterOrDigit() || text[i] == '_')) i--
        while (i >= 0 && text[i].isWhitespace()) i--
        val end = i + 1
        while (i >= 0 && (text[i].isLetterOrDigit() || text[i] == '_')) i--
        return text.subSequence(i + 1, end).toString() == "new"
    }

    /**
     * A type item accepted after `new`: its own insert handler (the auto-import)
     * runs first, then the constructor parentheses go in with the caret
     * between them -- `new Truck(<caret>)`, as Java writes it.
     */
    private fun constructorCall(element: LookupElement): LookupElement =
        com.intellij.codeInsight.lookup.LookupElementDecorator.withDelegateInsertHandler(element) { context, item ->
            item.handleInsert(context)
            val caret = context.editor.caretModel.offset
            val doc = context.document
            if (caret >= doc.textLength || doc.charsSequence[caret] != '(') {
                doc.insertString(caret, "()")
            }
            context.editor.caretModel.moveToOffset(caret + 1)
        }

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
