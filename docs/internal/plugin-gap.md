# Jux IntelliJ Plugin — Gap Analysis

**Author:** Claude (Opus 4.8) — audit of `ide/intellij-plugin`.
**Date:** 2026-06-11 · originally read-only; **resolution status added after the pro-level wave** (4 commits, suite 45/45).

## Resolution status (updated 2026-06-11, post-wave)

| Gap | Status | Note |
|-----|--------|------|
| Critical (LSP/Community) | ✅ Fixed | LSP4IJ 0.19.4 fallback client (`lsp4ij.xml` + `JuxLsp4ijServerFactory`, native-wins guard, shared `JuxLspCommandLine`); native gate switched `ultimate` → **`com.intellij.modules.lsp`** (covers IDEA free mode 2025.2+); description now truthful. `runIdeCommunity` gradle variant for CE smoke. |
| A1/A2 (ref coloring + roles) | ✅ Fixed | Decl-vs-reference coloring via `resolveLocally()` + call-shape; new keys: method call, param, local, type param, enum constant. |
| A3 (string interiors) | ✅ Fixed | `JuxStringAnnotator`: compiler-exact escape set (valid/invalid) + `${…}` holes **re-lexed** so embedded code colors properly. |
| C2/C3 (inspections+fixes) | ◐ Started | 3 native inspections + quick-fixes: unused/duplicate import (shared analysis with Ctrl+Alt+O), unused local/param/private-field (shadowing-aware), missing `@override`. More inspections remain future work. |
| C5 (false "cannot resolve") | ✅ Fixed+ | Refs made soft — and the audit undersold it: contributed references were **never surfaced at all** (leaf-targeted provider; custom-language PSI doesn't consult the registry). Now on composites with `getReferences()` override; rename/usages/cross-file type resolve actually work. |
| D2 (context completion) | ✅ Fixed | Keyword sets by grammar position (top-level/member/statement/expression), curated against the generated alphabet. |
| E2 (Go-to-Class/Symbol) | ✅ Fixed | `ChooseByNameContributorEx` pair over `JuxTypeIndex` walk + `ItemPresentation` on named elements. Stub index (B4) still deferred. |
| E3 (override gutters) | ◐ Up-arrows | Overrides/implements ↑ markers via `JuxHierarchy` (extracted, shared). Down-arrows need a reverse index — deferred with B4. |
| G1/G2 (formatter) | ✅ Fixed | Real block-model formatter (Ctrl+Alt+L): K&R indent/spacing tables, preserve-line-breaks v1, opaque runs protected, idempotent over all 144 corpus examples; Code Style | Jux page. |
| H4 (Run gutter) | ✅ Fixed | `RunLineMarkerContributor` on `main` (agrees with `JuxMainDetector`). |
| Parser drift (new) | ✅ Fixed | Corpus test was failing — 10 newer features unparsed: script-mode top level, tuples, leading method generics, `where`, multi-catch, or-patterns, try-expr, `out` args, `weak`, `operator ()`. All added. |
| Generate menu (new) | ✅ Fixed | Latent bug: actions targeted action id `Generate` instead of group `GenerateGroup` — never appeared in Alt+Insert. |
| Plugin ID (new) | ✅ Fixed | `verifyPlugin` rule: ID must not contain "intellij" → renamed `dev.jux.intellij` → `dev.jux.lang` (pre-release, no installed base). |
| D1/D3–D5 (smart completion) | ✅ Fixed (LSP wave, 2026-06-11) | `juxc-lsp` completion overhauled: **locals/params/enclosing-class members** (AST scope walker, `scope.rs`), **visibility-filtered** members (private/protected/package per Java rules), **static vs instance** receivers (`Type.` vs `obj.`), **enum variant** completion (`Color.`), **parameter snippets** (`greet(${1:who})`), **import-path completion**, `extends`/`implements`/`new` **type-position filtering** (non-final classes / interfaces / instantiable), no completions inside strings/comments, coherent `sort_text` ranking + `preselect`, doc comments via `completionItem/resolve`. IDE-side contributor now stands down when an LSP session is active (`JuxNativeLspStatus` probe + LSP4IJ plugin check) so the flat fallback never outranks the smart list. Fallback (no-LSP) keyword/in-file completion unchanged. |
| Language sync (new) | ✅ Fixed (2026-06-11) | Plugin caught up with the recent language surface: `static { }` blocks parse as STATIC_BLOCK (vs INIT_BLOCK); const generics `<int N>` decompose with `N` as the TYPE_PARAMETER; wrapping operators `+%`/`-%`/`*%`/`<<%`/`>>%` lex as single atoms, parse at their base ops' precedence, and space like them in the formatter; keyword completion gained `case`/`default`/`yield`/`sizeof` (statements/expressions), `extends`/`implements`/`permits` (headers), `native` (modifiers); folding for switch bodies + multiline raw strings; 10 new live templates (`prop`, `propgs`, `initb`, `sinit`, `drp`, `sw`, `swe`, `raws`, `opeq`). |
| Bug-hunt pass 2 (new) | ✅ Fixed (2026-06-11) | 16 findings from an adversarial sweep: **property `->` arrow** (parser accepted only `=>` — red errors on spec-legal `T name -> e;`); **`sizeof` as expression starter** (ternary `flag ? sizeof(int) : 4` misparsed as error-propagation; also C-style cast follow); lenient `yield expr;` statement; **Enter-handler `* ` injection** now requires real comment context (no more corrupting raw strings / wrapped multiplication); **interpolation-hole blindness** — names used only inside `$"${…}"` holes now count as usages for unused-import/unused-local (the quick-fixes deleted live code), plus case-pattern/opaque-run mentions suppress unused-local; raw-interp `\${x}` holes highlight (cooked `\$` correctly doesn't); `$name` shorthand highlights; lexer ends unterminated cooked holes at EOL (no more swallowing the file) and skips `\}` inside holes; `\xH"` invalid-escape range no longer bleeds over the quote; `\u{…}` validity matches `char::from_u32` (surrogates/out-of-range rejected); Generate actions now SEE `static` (statics no longer leak into generated ctors/getters); formatter keeps trailing same-line comments on `package`/`import` lines; const-generic head gated on primitive names (compiler lockstep); nested `type` aliases parse in class bodies; structure view stops listing params/locals and names constructors `new(...)`; `JuxTypeIndex` caches per-file type lists (override gutters were O(methods×supers×files) PSI walks per daemon pass). |
| B1–B4, C4/C6, E1/E4–E6, F, H1–H3/H5/H6, I1, J1–J4 | Open | Tracked below — refactoring surface, stub index, accessor/record PSI depth, param-info popup, injection, debugger/test-runner remain the next waves. Postfix templates (`.if`/`.var`) and override-method-name / annotation-name completion still open from D3/D4. |

## How this was derived

Read `META-INF/plugin.xml` (the full extension-point registration map) plus every
implementation file under `src/main/kotlin/dev/jux/intellij`. "Confirmed absent"
gaps are extension points **not registered** in `plugin.xml` (definitive). "Partial"
means a registered feature exists but is shallow. File:line anchors are given where
a specific implementation limit is the cause.

## What's already solid (so the gaps are in context)

The plugin is well past a skeleton. Present and real: a hand-written UTF-grade
**lexer** (fine-grained tokens, single-sourced from `juxc-lex` via `jux-tokens.json`),
a **full recursive-descent parser** — declarations *and* the complete statement +
expression grammar (do-while, C-for, labeled, `unsafe`, switch-as-expr, ranges,
type-test `=>`, lambdas, casts, full precedence cascade), a real **PSI** with typed
named elements, **Structure View**, **folding**, **brace match / quote / commenter /
Enter / indent**, **Optimize Imports**, **in-file** references (go-to-decl, find-usages,
rename), keyword + in-file completion, parameter-name **inlay hints**, **live
templates**, a **color settings page**, **Generate** (ctor/getters/setters/override),
New-File/Package/Project, a **tool window**, **run configurations**, toolchain
settings, and a **native LSP client** wiring `juxc-lsp`.

The gaps below are about depth and breadth, not absence of a foundation.

---

## The one structural gap that frames all others

**Almost every *semantic* feature is gated behind the Ultimate-only LSP.**
`plugin.xml:83` makes the LSP `optional` on `com.intellij.modules.ultimate`, and
`JuxLspServerSupportProvider` loads only there. Diagnostics, member completion (after
`.`), hover types, auto-import, cross-file navigation/rename — all come from
`juxc-lsp`. So on **Community IDEs** (IDEA CE, PyCharm CE, etc.) without LSP4IJ wired,
the plugin provides **zero diagnostics, zero member completion, zero cross-file
intelligence**. The description text mentions "or the LSP4IJ plugin on Community
editions," but `plugin.xml` has **no LSP4IJ integration** — only the native
ultimate API. **Gap:** either wire an LSP4IJ fallback, or build IDE-side semantics
(below) so Community isn't blank. This decision colors every category that follows.

---

## A. Syntax highlighting & colors

- **A1 — No decl-vs-reference coloring.** `JuxAnnotator` (`highlight/JuxAnnotator.kt:37-66`)
  colors only **declaration names**, annotation names, and primitive type names. Every
  *use* — a type reference, method call, field/variable/parameter read — stays the
  flat `IDENTIFIER` color. The plugin's own comments call this "Phase 5." This is the
  biggest visible highlighting gap: references look unstyled. *(Partial.)*
- **A2 — No distinct colors for many roles** the color page doesn't even declare:
  local variable, parameter, **method call** (vs declaration), static vs instance
  field, **type parameter / generic `T`**, enum constant *use*, label, reassigned
  variable, soft/contextual keywords (`var`, `get`/`set`, `out`, `move`). The
  descriptor set (`JuxColorSettingsPage.kt:25-46`) has ~20 entries, all
  declaration/lexical; Java's has ~60.
- **A3 — String interior is opaque.** The lexer emits an interpolated/raw string as
  **one token** (`JuxLexer.kt:21-22` doc). Consequences: **escape sequences** (`\n`,
  `\u{…}`) get no `VALID_STRING_ESCAPE` color and invalid escapes aren't flagged; and
  the embedded expressions in `$"…${expr}…"` get **no highlighting** at all.
- **A4 — No semantic/"rainbow" highlighting** for locals/params, no rainbow brackets.
- **A5 — `ColorDescriptor.EMPTY`** (`JuxColorSettingsPage.kt:21`) — no customizable
  background/scope colors (minor).
- **A6 — Annotation *argument* names** and annotation values aren't colored (the whole
  `(…)` is consumed paren-balanced, see B2).

## B. Parsing / PSI structure depth

The grammar *parses*, but several constructs are captured as opaque balanced runs,
so they have no inner PSI → no navigation/rename/structure/highlight inside them:

- **B1 — Property accessor bodies** are an opaque `CODE_BLOCK` (`JuxParser.kt:250-251`)
  — `get -> …` / `set -> …` aren't structured.
- **B2 — Annotation arguments** consumed paren-balanced (`JuxParser.kt:108`); **record
  components** consumed as one balanced blob (`parseRecordComponents`, `:315-319`,
  done as a single `RECORD_COMPONENT_LIST` with no per-component nodes); **enum
  constant payloads/discriminators** consumed balanced (`:190-191`); **operator
  symbol** consumed by skip-to-`(` (`:206-214`) — the operator token isn't a node.
  Each means: no go-to/rename/find-usages on those names, no per-item highlight.
- **B3 — Grouped imports** `import a.{ b, c as d }` consumed brace-balanced
  (`JuxParser.kt:69`) — the individual imported symbols aren't PSI, so per-symbol
  unused-import detection / optimize granularity / navigation is impossible.
- **B4 — No stub tree.** PSI is non-stubbed (`JuxParserDefinition` builds plain
  composite/named elements, no `StubElementType`). This blocks project-wide indices
  (see E2) and makes every cross-file query a full reparse.

## C. Diagnostics & inspections

- **C1 — No IDE-side diagnostics at all** off the LSP. The only `Annotator` is the
  highlighter (`JuxAnnotator`), which emits *silent* INFORMATION annotations only. So
  without Ultimate+LSP there is no error/warning highlighting. *(Confirmed.)*
- **C2 — No inspections** (`localInspection` / `inspectionToolProvider` not
  registered): no unused import/variable/field/parameter, unreachable code, redundant
  cast, missing-`@override`, always-true condition, etc. — none as native inspections.
- **C3 — No quick-fixes / intentions** (`intentionAction` not registered): no "import
  X", "create method/field", "make final", "add override stub", "remove unused". The
  Generate actions exist but aren't error-driven fixes.
- **C4 — Parser errors are generic strings** ("unexpected token", "'}' expected") not
  mapped to Jux **diagnostic codes** or severities — no link to the rich `Exxxx`/`Wxxxx`
  catalog the compiler owns.
- **C5 — Possible false "cannot resolve" on cross-file refs.** `JuxReference` is
  **non-soft** (`resolve/JuxReference.kt`; no `setSoft`), registered on
  `REFERENCE_EXPRESSION`/`TYPE_REFERENCE`/`FIELD_ACCESS`/`METHOD_REF`
  (`JuxReferenceContributor.kt:38-43`), and resolves **in-file only**. A reference to
  a cross-file/std symbol returns `null`. **Verify** whether the platform then paints a
  red "cannot resolve symbol" (false positive on valid code) or stays silent — if the
  former, this is an active bug; the in-file-only resolver should be soft.
- **C6 — No spellchecker** (`spellchecker.support` not registered) — comments/strings/
  identifiers aren't spell-checked.
- **C7 — TODO highlighting** should work (comment tokens are declared,
  `JuxParserDefinition.kt:31`) — listed only to confirm it's *not* a gap.

## D. Code completion

- **D1 — Member completion (after `.`) bails entirely** IDE-side
  (`JuxCompletionContributor.kt:39`, `isAfterDot` → return). It defers to the LSP, so
  on Community there is **no member completion**.
- **D2 — Keyword completion is context-blind** — every keyword is offered everywhere
  (`:41-43`), e.g. `class` mid-expression. No grammar-position filtering.
- **D3 — No completion for** import paths, package names, override-method names,
  annotation names, enum constants in a `switch`, or named arguments.
- **D4 — No smart/type-aware completion, no postfix templates** (`.if`, `.not`,
  `.var`), no parenthesis/argument insertion, no parameter info on accept.
- **D5 — Completion can't see other files** (in-file `PsiTreeUtil` walk, `:45-48`).

## E. Navigation, references & project-wide search

- **E1 — All cross-file navigation/usages/rename is LSP-only.** The IDE-side
  `JuxReference` resolves within the current file (`JuxReference.kt:29-69`); Find
  Usages and Rename are likewise in-file (`getVariants`/`handleElementRename`).
- **E2 — No Go-to-Class / Go-to-Symbol** (`gotoClassContributor` /
  `gotoSymbolContributor` not registered) — Ctrl+N / Ctrl+Alt+Shift+N don't find Jux
  types or members. Needs a stub index (B4).
- **E3 — No gutter icons for override/implement/overridden** (`lineMarkerProvider`
  not registered) — no "implements ↑ / overridden ↓" navigation markers.
- **E4 — No Type / Call / Method Hierarchy** providers; **no Go-to-Implementation /
  Go-to-Super-Method** (`definitionsScopedSearch` / `overridingMethodsSearch`).
- **E5 — No breadcrumbs** (`breadcrumbsInfoProvider` not registered).
- **E6 — Rename is leaf-swap only** (`JuxReference.kt:75-76`) — no conflict detection,
  no rename of file↔type, no cross-file rename without the LSP.

## F. Refactoring

**Confirmed absent — none registered.** No Move (class/file/package), Safe Delete,
Extract Variable/Method/Constant/Parameter/Field, Inline, Change Signature, Pull
Up/Push Down, Introduce. Only the Alt+Insert **Generate** actions exist (ctor,
getters, setters, override stubs). For a "modelled on the Java plugin" experience this
is the largest missing surface after diagnostics.

## G. Formatting & code style

- **G1 — No reformat (Ctrl+Alt+L).** No `FormattingModelBuilder` registered. Indent
  is heuristic only (`JuxLineIndentProvider` + `JuxEnterHandler`); there is no
  grammar-driven formatter, so "Reformat Code" does nothing meaningful.
- **G2 — No code-style settings** (`langCodeStyleSettingsProvider` /
  `codeStyleSettingsProvider` not registered): no tabs/spaces, brace placement, blank
  lines, wrapping, alignment, or import-layout configuration.
- **G3 — No Rearrange Code**, no format-on-paste.

## H. Editor intelligence

- **H1 — No Parameter Info popup (Ctrl+P)** — `lang.parameterInfo` not registered.
  (Inlay name hints exist, `hints/JuxInlayHintsProvider.kt`, but not the on-`(` popup;
  the LSP may cover signature help on Ultimate.)
- **H2 — No type inlay hints** for `var`/chained calls IDE-side (only parameter-name
  hints). LSP hover shows types but not inline.
- **H3 — No "Surround With"** (`lang.surroundDescriptor`) — no surround with
  if/try/for.
- **H4 — No Run gutter icon.** No `RunLineMarkerContributor` registered; running is via
  the context `RunConfigurationProducer` (right-click / Ctrl+Shift+F10) and tool
  window — but the always-visible green-arrow gutter next to `main` isn't there.
  *(Verify against intent — plugin.xml description implies a gutter Run.)*
- **H5 — No language injection** (`multiHostInjector`) — can't inject regex/SQL/JSON
  into strings, and (with A3) the `${…}` interpolation holes get no embedded language
  support.
- **H6 — No Move-Statement-Up/Down**, no smart-complete-statement (Ctrl+Shift+Enter)
  beyond the Enter handler, no structural search/replace.

## I. Documentation & hover

- **I1 — `JuxDocumentationProvider`** renders a declaration's signature + doc comment
  from PSI, **in-file only** (`documentation/`). No cross-file quick-doc, no std/library
  doc (LSP hover only on Ultimate), no rendered-markdown, no per-parameter doc, no
  external-doc links, no quick-doc for keywords/builtins.

## J. Run / build / project / debug

- **J1 — No debugger.** No `xdebugger`/debug configuration. Jux→Rust would need a
  native (lldb/gdb) debug bridge with source-mapping back to `.jux` — entirely absent.
- **J2 — No test-runner integration.** `jux test` runs as raw console output in the
  tool window; no green/red test tree, no per-test gutter Run, no re-run-failed.
- **J3 — No real build-system model.** No project sync, dependency/library model, or
  SDK — the tool window shells out to the toolchain; cross-module resolution is
  therefore LSP-only (ties back to the Community gap).
- **J4 — No coverage, no profiler** integration.

---

## Priority summary

| Area | Gap | Severity | Status |
|------|-----|----------|--------|
| LSP/Community | Semantics gated behind Ultimate-only LSP; no LSP4IJ fallback | **Critical** | Confirmed |
| Highlight | No decl-vs-reference coloring (refs look unstyled) | High | Partial (A1) |
| Diagnostics | No IDE-side errors/inspections without LSP | High | Confirmed |
| Formatting | No reformat / code-style settings | High | Confirmed |
| Refactoring | No move/extract/inline/change-signature/safe-delete | High | Confirmed |
| Navigation | No Go-to-Class/Symbol, no stub index, no hierarchies/gutters | High | Confirmed |
| Resolve | In-file only; possible false "cannot resolve" on cross-file (C5) | Med-High | Verify |
| Completion | No member/context/postfix completion IDE-side | Medium | Partial |
| PSI | Opaque runs (record comps, annotation args, accessors, grouped imports) | Medium | Partial |
| Strings | No escape highlighting; interpolation holes are dead text | Medium | Confirmed |
| Editor | No param-info popup, surround, injection, run-gutter, type hints | Medium | Confirmed |
| Run/Debug | No debugger, no test-runner UI, no build-system model | Medium | Confirmed |
| Docs | Quick-doc in-file only | Low | Partial |
| Spellcheck | None | Low | Confirmed |

## Big-picture take

The plugin has an **excellent syntactic foundation** (real lexer + full PSI parser +
the editor-behavior layer) that most language plugins never finish. The gaps cluster
in **two bands**:

1. **Semantic features that currently exist only through the Ultimate LSP** —
   diagnostics, completion-after-dot, cross-file navigation/rename, hover types. The
   single highest-leverage decision is the **Community story**: wire LSP4IJ, or invest
   in IDE-side semantics. Today a Community user gets a pretty editor with no error
   checking.
2. **Whole IDE surfaces not yet started** — **formatting/code-style**,
   **refactoring**, **project-wide symbol search (stub index)**, **inspections +
   quick-fixes**, **hierarchies/gutters**, and **debug/test-runner**. These are the
   difference between "syntax support" and the "modelled on the Java plugin"
   experience the description promises.

The nearest-term, highest-value items that **don't** depend on the LSP: **A1**
(reference coloring — the most visible polish), **C5** (confirm cross-file refs don't
false-error), **G1** (a formatter), and **E2/B4** (a stub index unlocking Go-to-Symbol
and faster cross-file work).
