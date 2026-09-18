# Jux Spec Addendum — Language Server (`juxc-lsp`)

**Status:** Proposed insertion. Specifies the Jux Language Server: its crate layout, transport, dependency surface, capability matrix, re-analysis model, and the mapping from `juxc-diag` diagnostics to LSP `Diagnostic` objects. Companion to `JUX-EDITOR-TOOLING-ADDENDUM.md`, which covers the TextMate grammar and per-editor integration; this file covers the server itself.

**Insertion points:**
- New §L.1 ("Goals and Non-Goals")
- New §L.2 ("Crate Layout")
- New §L.3 ("Transport")
- New §L.4 ("Dependencies")
- New §L.5 ("Server Capabilities")
- New §L.6 ("Re-analysis Model")
- New §L.7 ("Diagnostic Mapping")
- New §L.8 ("Position Encoding")
- New §L.9 ("Implementation Phases")
- New §L.10 ("Open Questions")

---

## Current State (2026-09-18)

> **Read this first.** The sections below were written as a plan. This one
> says what the server does today; where they disagree, this section wins.

- **Capabilities.** Every capability in §L.5 except `documentFormattingProvider`
  is implemented: incremental sync, diagnostics, hover, goto-definition,
  document symbols, references, completion (with `completionItem/resolve`),
  signature help, code actions (auto-import), rename with `prepareRename`,
  semantic tokens (full and range), inlay hints, and `workspace/symbol`.
- **Position encoding (§L.8).** Negotiated at `initialize`: UTF-8 when the
  client offers it in `general.positionEncodings`, UTF-16 otherwise, and
  echoed back in `ServerCapabilities.positionEncoding`. The per-line
  conversions live in `juxc_source::position`.
- **Source roots.** The server reads the editor's `jux.sourceRoots`
  (JUX-INTELLIJ-PLUGIN-ADDENDUM §I.4, "Coordination with `juxc-lsp`"), see
  §L.6.
- **Crate layout.** `server.rs` holds the lifecycle and the document store
  only; each feature is a module of plain functions over a document (§L.2).
- **Still open.** `textDocument/formatting` (needs `juxc fmt`); semantic-token
  deltas (`semanticTokens/full/delta`); the backend-free dependency set of
  §L.2 (see "Crate dependencies inside the workspace").

---

## Design Philosophy (Non-Normative)

`juxc` is already a multi-crate Rust workspace (`juxc-lex`, `juxc-parse`, `juxc-ast`, `juxc-tycheck`, `juxc-diag`, …) with first-class diagnostics. The cheapest path to good editor support is therefore **to reuse the existing front end** behind an LSP shim, not to rebuild it inside an IDE plugin.

Consequences:

1. **One server, many editors.** The Language Server Protocol was designed for exactly this case. We commit to LSP as the primary semantic-tooling surface; native IDE plugins are scoped to thin launcher shells where the host editor cannot otherwise discover the server.
2. **No duplication of the front end.** `juxc-lsp` MUST NOT contain its own parser, type checker, or symbol table. Every semantic answer comes from the same crates that drive the batch compiler. If a question cannot be answered by the existing crates, the right fix is to expose the API on the crate, not to re-implement it in the server.
3. **Synchronous first, incremental later.** Phase 1 re-analyses on every edit at whole-file granularity. Incremental tycheck is a Phase 3 concern and is gated by demonstrated latency problems, not by speculation.

What `juxc-lsp` explicitly **does not** do:

- It does not invoke `rustc` or shell out to the lowering pass. Diagnostics come from `juxc-tycheck`, not from compile errors in emitted Rust. (Lowering-stage errors are a separate, deferred feature — see §L.10.)
- It does not maintain its own on-disk index. Cross-file information is rebuilt from the in-memory `DashMap<Url, Document>` plus the project's `jux.toml` manifests.
- It does not implement code formatting itself. `textDocument/formatting` delegates to the `juxc fmt` binary (see §L.5, phase 3).

---

## §L.1 — Goals and Non-Goals

### Goals (Phase 1)

| # | Capability                                                | LSP method                              |
|---|-----------------------------------------------------------|------------------------------------------|
| 1 | Real-time diagnostics (E0xxx codes from `juxc-tycheck`)   | `textDocument/publishDiagnostics`       |
| 2 | Hover: type of expression under cursor                    | `textDocument/hover`                    |
| 3 | Goto-definition / goto-declaration                        | `textDocument/definition`               |
| 4 | Find references                                           | `textDocument/references`               |
| 5 | Document symbols (outline view)                           | `textDocument/documentSymbol`           |
| 6 | Basic completion (keywords, in-scope idents, members)     | `textDocument/completion`               |
| 7 | Rename refactor (textual-aware; semantic in phase 3)      | `textDocument/rename`                   |
| 8 | Semantic tokens (overrides TextMate coloring)             | `textDocument/semanticTokens`           |

### Non-Goals (Phase 1)

- Debug Adapter Protocol (DAP) support. Debugging is owned by the runtime/ABI addenda, not this server.
- Notebook-document support (`notebookDocument/*` LSP methods).
- Inline values and code lens: phase 3 or later.

Workspace symbol search and inlay hints were listed here once; both are
implemented (§L.5).

---

## §L.2 — Crate Layout

```
crates/juxc-lsp/src/
├── main.rs         # binary entry; wires up the tower-lsp Server
├── server.rs       # LanguageServer impl: lifecycle, document store, routing
├── doc.rs          # one open document: its rope and its analysis caches
├── analysis.rs     # runs the front end over the compilation set of a file
├── sync.rs         # incremental didChange edits applied to the rope
├── position.rs     # LSP positions in the negotiated encoding
├── roots.rs        # the editor's jux.sourceRoots
├── diagnostics.rs  # juxc diagnostics to LSP diagnostics (§L.7)
├── capabilities.rs # the advertised capability set, asserted by a test
├── xref.rs         # which declaration each identifier denotes
├── hover.rs        # textDocument/hover
├── definition.rs   # textDocument/definition
├── references.rs   # textDocument/references, prepareRename, rename
├── symbols.rs      # textDocument/documentSymbol, workspace/symbol
├── completion.rs   # textDocument/completion, completionItem/resolve
├── calls.rs        # signature help, and the callee lookup inlay hints share
├── semtok.rs       # textDocument/semanticTokens
├── inlay.rs        # textDocument/inlayHint
├── code_action.rs  # the auto-import quick fix
├── imports.rs      # reading and inserting import lines
├── intel.rs        # rendering and member lookup over the symbol table
├── scope.rs        # locals visible at the caret, for completion
├── text.rs         # byte scans over raw text (word at cursor, doc comments)
└── workspace.rs    # the whole-project index
```

Every handler is a plain function over a document, so tests drive it without
an LSP client; `server.rs` only finds the document and routes the request.

### Binary name

The produced binary MUST be named `juxc-lsp` (no `.exe` distinction at the crate level; Cargo handles the platform suffix). Editors discover it via `$PATH` or via an explicit path in their server configuration.

### Crate dependencies inside the workspace

`juxc-lsp` depends on `juxc-lex`, `juxc-parse`, `juxc-ast`, `juxc-tycheck`, `juxc-diagnostics`, and `juxc-source`. It MUST NOT add a new dependency on `juxc-codegen` or any lowering crate: those are batch-mode only.

**Pending.** The server also depends on `juxc-driver`, for the backend-free
check entry (`check_workspace_cfg`) and for generating crate stubs
(`ensure_project_stubs`). The driver depends on `juxc-backend-rust`, so the
lowering crate still reaches the server's build transitively, although no
server code calls it. The stub generator uses three Cargo-manifest helpers of
the backend (`CrateSource`, `registry_dep_line_with`, `registry_dep_line_for`)
and the manifest types use `CargoProfile`/`CargoMeta`. The clean cut is to move
those Cargo-manifest types and helpers out of `juxc-backend-rust` into a small
crate both the backend and the driver use, then gate the driver's lowering
entry points behind a default `codegen` feature that `juxc-lsp` turns off.

---

## §L.3 — Transport

`juxc-lsp` MUST speak LSP over **stdio** by default. JSON-RPC messages are framed with the standard `Content-Length:` headers per the LSP 3.17 specification.

A `--socket <port>` flag MAY be added for editors that prefer TCP (primarily for debugging the server itself with an inspector attached). No HTTP, no WebSocket transport is supported in Phase 1.

The server MUST NOT log to stdout. All logging goes to stderr or to the LSP `window/logMessage` channel — writing to stdout corrupts the JSON-RPC stream.

---

## §L.4 — Dependencies

Recommended crates:

| Crate          | Purpose                                                    |
|----------------|------------------------------------------------------------|
| `tower-lsp`    | Async LSP scaffolding — framing, lifecycle, request routing |
| `tokio`        | Async runtime (implied by `tower-lsp`)                      |
| `dashmap`      | Concurrent `Url → Document` store                          |
| `ropey`        | Rope structure for fast incremental edits to text buffers   |
| `lsp-types`    | LSP type definitions                                       |
| `serde`        | Already in the workspace                                   |
| `serde_json`   | Already in the workspace                                   |

Alternative scaffolding (`async-lsp`, `lsp-server` from `rust-analyzer`) is acceptable if a future contributor presents a clear advantage; `tower-lsp` is the default because it is the most-documented choice in 2026.

### Hard rule

`juxc-lsp` MUST NOT depend on `rustc`, `rustc_*` crates, or any LLVM bindings. All semantic information comes from the Jux front-end crates.

---

## §L.5 — Server Capabilities

The server advertises the following in its `InitializeResult.capabilities`. The "Phase" column refers to §L.9.

| Capability                       | LSP method                              | Phase |
|----------------------------------|------------------------------------------|-------|
| `textDocumentSync` (incremental) | `textDocument/didOpen` / `…/didChange` | 1     |
| `diagnosticProvider`             | `textDocument/publishDiagnostics`       | 1     |
| `hoverProvider`                  | `textDocument/hover`                    | 1     |
| `definitionProvider`             | `textDocument/definition`               | 1     |
| `documentSymbolProvider`         | `textDocument/documentSymbol`           | 1     |
| `referencesProvider`             | `textDocument/references`               | 2     |
| `completionProvider`             | `textDocument/completion`               | 2     |
| `semanticTokensProvider`         | `textDocument/semanticTokens`           | 2     |
| `renameProvider`                 | `textDocument/rename`                   | 3     |
| `documentFormattingProvider`     | `textDocument/formatting`               | 3     |
| `inlayHintProvider`              | `textDocument/inlayHint`                | 3     |

### `textDocumentSync` — incremental

The server requests **incremental** sync (`TextDocumentSyncKind::Incremental`). Documents are stored as `ropey::Rope`; `didChange` edits are applied to the rope in O(log n), in order, the moment they arrive (see §L.6 for how the analysis follows).

### `references` and `rename`

A name resolves to one of three targets: a **local** (a parameter or local
variable, identified by its declaring identifier), a **member** of a type
(identified by the declaring type and the name; enum variants included), or a
**top-level declaration** (a type, function, constant or alias, identified by
its declaring unit and position). Resolution follows the checker's scoping: the
innermost visible binding wins; a bare name that is no binding is a member of
the enclosing type when it declares one (implicit `this`), else a top-level
declaration, preferring an explicit import, then a wildcard import, then the
file's own package; after a `.` the receiver's type decides (`this`, `super`,
a type name for statics, else the expression's inferred type).

`textDocument/references` returns every identifier across the project's files
that resolves to the same target (a local: its own file only), honoring
`includeDeclaration`. Names inside `${…}` interpolation holes count.

`textDocument/rename` edits exactly those occurrences, and refuses rather than
half-renames when:

- the new name is not an identifier, is a keyword, or is a built-in type name;
- the declaration lives outside the project (the standard library, a generated
  `.jux.d` stub);
- the new name already means something where the old one is used (another local
  in scope, an existing member of the type, an existing declaration in the
  package).

`textDocument/prepareRename` reports the name's range and text, or the reason it
cannot be renamed. A type declared in the file named after it (LANG-V1 §3.1)
takes its file along in the same workspace edit when the client supports file
renames (`workspaceEdit.documentChanges` with `resourceOperations` including
`rename`).

### `semanticTokens` legend

Token types: `namespace`, `type`, `class`, `enum`, `interface`, `struct`,
`typeParameter`, `parameter`, `variable`, `property`, `enumMember`, `function`,
`method`. Modifiers: `declaration`, `static`, `readonly`, `defaultLibrary`. Only
identifiers are emitted, classified by the same resolution as references; a
record reads as `struct`, a constant as `variable` + `readonly`, anything
declared in the standard library or a generated stub carries
`defaultLibrary`. Keywords, literals and comments stay with the grammar.

### `inlayHint`

Two kinds: the inferred type after a `var` local's name (`: int`), and the
parameter name before each positional argument (`width:`). A parameter hint is
left out when the argument is already that name, when the argument is named,
and when the argument count does not single out one overload.

### `workspace/symbol`

Every type, function, constant and member declared in the project's own files
(not the standard library or generated stubs) whose name matches the query as
an in-order, case-insensitive subsequence (`shcart` finds `ShoppingCart`),
prefix matches first, at most 512. The source is the last whole-project index
(§L.6), so no separate on-disk index is kept.

### `definition` — goto-definition

`textDocument/definition` jumps to the declaration of the identifier under the cursor — a **type, free function, constant, or type alias** name. The symbol table records `decl_unit: FQN → declaring-unit-index` during its build pass; the LSP resolves the identifier to its canonical FQN (exact key, else last-segment match, preferring a non-external type so an unqualified `Box`/`HashMap` lands on the Jux type, not the `rust.std` stub), reads the matching `*Sig::span`, and pairs it with the analysed `source_paths[unit]` to form the `Location`. This reaches **into generated `rust.std` / crate `.jux.d` stubs** (the cached stub file is a real, openable path), so "go to definition" on a Rust-derived type opens its Jux-syntax declaration. Member-level goto (`recv.method`) awaits per-member source spans; today the receiver still resolves for hover/completion.

### `semanticTokens` precedence

When `semanticTokens` is enabled and returns a non-empty token map for a document, the editor's LSP client overrides the TextMate-grammar classification for that range. This is the channel through which we resolve "is this identifier a type, a value, or a function?" — questions TextMate cannot answer.

### `completion` triggers

The `CompletionOptions.triggerCharacters` MUST include at least `.`, `:`, `@`. `.` triggers member completion, `:` triggers `::` path completion, `@` triggers annotation-name completion.

---

## §L.6 — Re-analysis Model

### On `didOpen`

Full lex → parse → tycheck pass on the opened document. Cache the resulting AST and tycheck context keyed by `Url`. Publish diagnostics.

### On `didChange`

1. Apply the edits to the document's `Rope` immediately, and record the new version.
2. Re-run analysis for that revision on a blocking thread.
3. Install the result only if the document still holds that revision; a pass for an older revision is dropped without publishing, because the newer revision's own pass is already on its way. This keeps a slow analysis from overwriting a newer one's caches.
4. Publish the new diagnostics for every file the pass analysed.

Every pass reads the other open buffers' live text rather than their files on
disk, so an unsaved edit in one buffer is what the analysis of another sees,
and cross-file ranges (references, rename) match the text in the editor.

### The compilation set, and source roots

A file is analysed together with the files of its project. By default that is
every `.jux` file under the nearest `jux.toml` (widened to an enclosing
`[workspace]`), or the file alone when no manifest governs it.

When the editor sends source roots (`jux.sourceRoots`, a list of
`{ "path", "kind": "sources" | "tests", "origin": "marked" | "manifest" }`),
a file under a root is analysed with every `.jux` file under the roots that
share its root's governing `jux.toml` (or, for roots no manifest governs,
with every such root). A project with marked roots and no manifest thereby
gets cross-file analysis, and a manifest project's files outside its roots
(`examples/`, scratch files) stay out of its analysis. The file's location
under its root also gives its package where the file declares none, which
completion and the auto-import fix use for same-package decisions.

The server takes the roots from the initialization option
`{ "jux": { "sourceRoots": [...] } }`, from `workspace/didChangeConfiguration`
carrying the same object, and, when neither has been sent (or a configuration
change carries no settings), by asking with `workspace/configuration` for the
section `jux.sourceRoots`. A change re-indexes the project and re-analyses the
open documents.

### On `didSave`

No special behavior in Phase 1. (Phase 3 may use save as a trigger for slower work like cross-module rename validation.)

### Whole-workspace re-tycheck

Out of scope for Phase 1. The reasoning: typical Jux projects are small (Phase 1 ships before a package ecosystem), and the cost of caching invalidation logic exceeds the cost of recomputing on demand. Revisit if workspaces over 1,000 files become common.

---

## §L.7 — Diagnostic Mapping

`juxc-diag` already produces structured diagnostics. The mapping to `lsp_types::Diagnostic` is one-to-one:

| `juxc-diag` field     | LSP `Diagnostic` field    | Notes                                                            |
|-----------------------|---------------------------|------------------------------------------------------------------|
| `code` (e.g. `E0420`) | `code` (`String` variant) | Surfaced in the editor as a clickable code linking to docs       |
| `severity`            | `severity`                | `Error` / `Warning` / `Information` / `Hint`                     |
| `span.start..end`     | `range`                   | Translated via §L.8                                              |
| `message`             | `message`                 | Primary diagnostic text                                          |
| `labels`              | `relatedInformation`      | Each label, located in the file its span indexes (a "first declared here" label often points into another file); a label in a file the editor cannot open (the standard library) joins the message as a `note:` line instead |
| `notes` / `help`      | `message`                 | Spanless, so folded into the message as `note:` / `help:` lines, where every client shows them (IntelliJ's LSP client does not render `relatedInformation`) |
| (constant)            | `source: "juxc"`          | Lets editors group Jux diagnostics distinctly from other tooling |

A future `codeDescription.href` field MAY be populated once the diagnostic catalog has stable URLs (e.g. `https://juxlang.dev/diagnostics/E0420`).

---

## §L.8 — Position Encoding

LSP positions are **UTF-16 code units** by default, not byte offsets. `juxc-source::Span` stores **UTF-8 byte offsets**. The server MUST translate between the two on every position-bearing message.

The server SHOULD advertise `positionEncodings: ["utf-8", "utf-16"]` in its `ServerCapabilities` (LSP 3.17+) and prefer UTF-8 if the client accepts it. Most modern editors (VS Code 1.81+, recent IntelliJ, Zed, Helix) negotiate UTF-8. The UTF-16 fallback path is required for compatibility with editors that have not yet adopted the negotiation.

Translation helpers live in `juxc-source` (extend the existing crate, do not duplicate logic in `juxc-lsp`).

**Implemented.** `juxc_source::position` holds the per-line conversions
(`byte_to_col`, `col_to_byte`) and a whole-text `LineIndex`, for both
encodings; `juxc-lsp` finds the line in its rope and calls them. A column past
a line's end clamps to the line end, and a column inside a character (half a
surrogate pair, the middle of a UTF-8 sequence) lands on the character's
start.

---

## §L.9 — Implementation Phases

| Phase | Deliverable                                                              | Editors unlocked end-to-end                     |
|-------|---------------------------------------------------------------------------|-------------------------------------------------|
| 1     | `crates/juxc-lsp` with capabilities marked Phase 1 in §L.5                | VS Code, IntelliJ Ultimate, Zed, Neovim, Helix |
| 2     | Capabilities marked Phase 2: references, completion, semanticTokens       | Same editors, richer experience                 |
| 3     | Capabilities marked Phase 3: rename, formatting, inlayHints               | Refactoring-grade IDE experience                |
| 4     | Packaged extensions (VS Code Marketplace + JetBrains plugin) that ship + launch the server | One-click install on the two dominant IDEs |
| 5     | Incremental tycheck (if Phase 1 latency proves inadequate)                | No new features; latency improvement only       |

---

## §L.10 — Open Questions

- **Lowering-stage diagnostics.** `juxc` can produce errors during AST-to-Rust lowering (e.g. a feature reaches the lowering pass that tycheck didn't reject because the constraint was post-tycheck). Should the LSP run the lowering pass too? Probably yes for paranoia, but the latency cost is unknown. Deferred until lowering passes are stable enough to benchmark.
- **WASM build.** Compiling `juxc-lsp` to `wasm32-unknown-unknown` for an in-browser Jux playground (Monaco + a WASM LSP) is plausible — the front end has no `std::fs` requirement that would forbid it. Scope-deferred to a separate playground addendum.
- **Workspace symbols.** Resolved without a new crate: the server keeps the symbol table of its last whole-project index pass (which it already runs for completion) and searches that (§L.5).
- **Multi-root workspaces.** LSP supports multiple workspace folders. The server indexes from the first one; the editor's source roots (§L.6) already group files by their own `jux.toml`, so several projects under one folder analyse separately. A second workspace folder is still not indexed.
