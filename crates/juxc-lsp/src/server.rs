//! The `LanguageServer` implementation: lifecycle, the document store, and the
//! request routing.
//!
//! Every request handler here is thin: it finds the document and hands it to
//! the module that owns the feature (`hover`, `definition`, `completion`,
//! `calls`, `references`, `semtok`, `inlay`, `symbols`, `code_action`), each a
//! plain function over a [`Document`] that tests drive without a client.
//!
//! What lives here is the state that spans requests:
//!
//! - the open documents, whose text is edited in place by incremental
//!   `didChange` (§L.5) and whose analysis caches are refreshed behind it;
//! - the project index (for completion, auto-import and `workspace/symbol`);
//! - the position encoding negotiated at `initialize` (§L.8);
//! - the editor's source roots (`jux.sourceRoots`, plugin addendum §I.4),
//!   which decide what an open file is analysed together with.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::RwLock;

use dashmap::DashMap;
use ropey::Rope;
use serde_json::Value;
use tower_lsp::jsonrpc::{Error as RpcError, Result};
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::analysis::{analyze_single_in, analyze_workspace_in};
use crate::completion::{build_completions, resolve_item, DocSnapshot};
use crate::doc::Document;
use crate::position::{negotiate, PositionEncoding};
use crate::roots::SourceRoots;
use crate::workspace::Workspace;

/// The configuration section the editor keeps its source roots under.
const ROOTS_SECTION: &str = "jux.sourceRoots";

/// Is the process with id `pid` still alive? Used by the parent-process
/// heartbeat. Dependency-free: on Windows it queries the process exit code via
/// `kernel32`; elsewhere it conservatively returns `true` (those platforms
/// rely on the stdin-EOF exit path instead).
#[cfg(windows)]
fn parent_alive(pid: u32) -> bool {
    use std::os::raw::c_void;
    // Minimal kernel32 bindings, which avoids pulling in a winapi crate.
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn GetExitCodeProcess(handle: *mut c_void, code: *mut u32) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;
    const STILL_ACTIVE: u32 = 259;
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false; // can't open → treat as gone
        }
        let mut code: u32 = 0;
        let ok = GetExitCodeProcess(handle, &mut code);
        CloseHandle(handle);
        ok != 0 && code == STILL_ACTIVE
    }
}

#[cfg(not(windows))]
fn parent_alive(_pid: u32) -> bool {
    true
}

/// The language server backend: an `LspService`-managed handler plus the
/// in-memory document store.
pub struct Backend {
    client: Client,
    /// One [`Document`] per open buffer. `DashMap` gives concurrent access
    /// without a global lock (§L "Re-analysis Model").
    docs: DashMap<Url, Document>,
    /// Project-wide index (root, all module type names, the last whole-project
    /// symbol table) for completion, auto-import and `workspace/symbol`.
    workspace: RwLock<Workspace>,
    /// URIs we last published a non-empty diagnostic set for. A workspace
    /// check reports diagnostics for *several* files at once; when a later
    /// check finds a file clean, we must publish an empty list to clear it.
    published: DashMap<Url, ()>,
    /// The column unit negotiated at `initialize` (§L.8). UTF-16 until then.
    encoding: RwLock<PositionEncoding>,
    /// The editor's source roots (§I.4). Empty until the client sends them.
    roots: RwLock<SourceRoots>,
    /// Whether the client applies file renames inside a workspace edit, which
    /// renaming a type in the file named after it needs.
    file_renames: AtomicBool,
}

impl Backend {
    /// Construct the backend with its LSP client handle. Matches the
    /// `Fn(Client) -> Backend` shape `LspService::new` expects.
    pub fn new(client: Client) -> Self {
        Self {
            client,
            docs: DashMap::new(),
            workspace: RwLock::new(Workspace::default()),
            published: DashMap::new(),
            encoding: RwLock::new(PositionEncoding::Utf16),
            roots: RwLock::new(SourceRoots::default()),
            file_renames: AtomicBool::new(false),
        }
    }

    fn encoding(&self) -> PositionEncoding {
        self.encoding.read().map(|e| *e).unwrap_or_default()
    }

    fn roots(&self) -> SourceRoots {
        self.roots.read().map(|r| r.clone()).unwrap_or_default()
    }

    /// Generate (or refresh) the project's foreign-crate (`rust.*` / `c.*` /
    /// `cpp.*`) dependency stubs under `.jux-stubs/`, so the bound crates' APIs
    /// autocomplete and auto-import in Jux syntax without a prior `jux build`
    /// (JUX-BINDGEN §G.6/§G.10/§G.11). The workspace scan already indexes
    /// `.jux-stubs/`; this fills it for every `rust.<crate>` declared across the
    /// project and its modules.
    ///
    /// rustdoc generation shells out and only runs for a stub that's missing, so
    /// this is invoked once on project open. It's CPU/process-bound, so it runs
    /// on a blocking thread; failures (offline, no nightly) are logged, never
    /// surfaced as diagnostics. No-op until a root is set.
    async fn ensure_crate_stubs(&self) {
        let Some(root) = self.workspace.read().ok().and_then(|ws| ws.root.clone()) else {
            return;
        };
        let report =
            match tokio::task::spawn_blocking(move || juxc_driver::ensure_project_stubs(&root))
                .await
            {
                Ok(r) => r,
                Err(_) => return, // the blocking task panicked; skip
            };
        for w in &report.warnings {
            self.client
                .log_message(MessageType::WARNING, format!("jux: {w}"))
                .await;
        }
        // A stub that failed to generate means that crate's types/members won't
        // autocomplete and will surface as "unresolved": surface it visibly (not
        // just in the log) so the cause (missing nightly / `rust-docs-json` /
        // network) is discoverable rather than mysterious.
        if !report.warnings.is_empty() {
            self.client
                .show_message(
                    MessageType::WARNING,
                    format!(
                        "jux: {} crate stub(s) couldn't be generated -- those crates \
                         won't autocomplete. See the output log for details \
                         (a nightly toolchain with the `rust-docs-json` component \
                         and network access is required).",
                        report.warnings.len()
                    ),
                )
                .await;
        }
        if !report.resolved.is_empty() {
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("jux: indexed {} bound-crate stub(s)", report.resolved.len()),
                )
                .await;
        }
    }

    /// The live text of every open buffer, keyed by path, so an analysis sees
    /// unsaved edits in files other than the one it is for.
    fn open_buffers(&self) -> HashMap<PathBuf, String> {
        let mut out = HashMap::new();
        for entry in self.docs.iter() {
            if let Ok(path) = entry.key().to_file_path() {
                out.insert(path, entry.value().rope.to_string());
            }
        }
        out
    }

    /// Re-scan every `.jux` file in the project and refresh the workspace
    /// index. Runs the (heavy) analysis on a blocking thread; cheap to call on
    /// open/save. No-op until a root is set.
    async fn reindex(&self) {
        let root = match self.workspace.read() {
            Ok(ws) => ws.root.clone(),
            Err(_) => return,
        };
        let Some(root) = root else { return };
        let overrides = self.open_buffers();
        let index =
            tokio::task::spawn_blocking(move || crate::workspace::index_workspace(&root, &overrides))
                .await
                .unwrap_or_default();

        if let Ok(mut ws) = self.workspace.write() {
            ws.type_names = index.type_names;
            ws.member_names = index.member_names;
            ws.type_packages = index.type_packages;
            ws.symbols = index.symbols;
            ws.source_paths = index.source_paths;
            ws.source_texts = index.source_texts;
        }
    }

    /// Re-analyse the open document at `uri`, install the result, and publish
    /// diagnostics. Called after open and after every change.
    ///
    /// The analysis (lex → parse → resolve → tycheck over the whole project +
    /// stdlib) is CPU-bound and runs on every keystroke, so it goes on a
    /// blocking thread. Edits keep arriving meanwhile and are applied to the
    /// rope directly; this analysis is installed only if the buffer still holds
    /// the revision it analysed (see [`Document::install`]), and a stale pass
    /// publishes nothing: the newer revision's own pass will.
    async fn refresh(&self, uri: Url) {
        let (rope, version) = match self.docs.get(&uri) {
            Some(doc) => (doc.rope.clone(), doc.version),
            None => return,
        };
        let root = self.workspace.read().ok().and_then(|ws| ws.root.clone());
        let roots = self.roots();
        let enc = self.encoding();
        let mut overrides = self.open_buffers();
        if let Ok(path) = uri.to_file_path() {
            overrides.remove(&path);
        }

        let task_uri = uri.clone();
        let task_rope = rope.clone();
        let task_roots = roots.clone();
        let analysis = match tokio::task::spawn_blocking(move || match root {
            Some(root) => analyze_workspace_in(&root, &task_uri, &task_rope, &task_roots, &overrides, enc),
            None => analyze_single_in(&task_uri, &task_rope, enc),
        })
        .await
        {
            Ok(a) => a,
            Err(_) => return, // the blocking task panicked; skip this revision
        };

        let diagnostics = analysis.diagnostics_by_uri.clone();
        let installed = match self.docs.get_mut(&uri) {
            Some(mut doc) => {
                let ok = doc.install(version, analysis);
                if ok {
                    doc.inferred_package = uri
                        .to_file_path()
                        .ok()
                        .and_then(|p| roots.package_of(&p));
                }
                ok
            }
            None => false,
        };
        if !installed {
            return;
        }

        // Publish diagnostics PER FILE. A workspace check can surface errors
        // in files other than the open one, so each file's group goes under its
        // own URI. Files reported dirty before that this pass found clean (or
        // didn't analyse) get an empty list: publishing one is how LSP clears.
        let mut now_dirty: Vec<Url> = Vec::new();
        for (file_uri, diags) in diagnostics {
            if !diags.is_empty() {
                now_dirty.push(file_uri.clone());
            }
            // Version only applies to the open document; other files get None.
            let ver = if file_uri == uri { Some(version) } else { None };
            self.client.publish_diagnostics(file_uri, diags, ver).await;
        }
        let stale: Vec<Url> = self
            .published
            .iter()
            .map(|e| e.key().clone())
            .filter(|u| !now_dirty.contains(u))
            .collect();
        for u in stale {
            self.published.remove(&u);
            self.client.publish_diagnostics(u, Vec::new(), None).await;
        }
        for u in now_dirty {
            self.published.insert(u, ());
        }
    }

    /// Adopt `roots` if they differ from the current set, then re-index and
    /// re-analyse every open document under the new layout. Returns whether
    /// anything changed.
    async fn set_roots(&self, roots: SourceRoots) -> bool {
        let changed = match self.roots.write() {
            Ok(mut current) if *current != roots => {
                *current = roots;
                true
            }
            _ => false,
        };
        if changed {
            self.reindex().await;
            let open: Vec<Url> = self.docs.iter().map(|e| e.key().clone()).collect();
            for uri in open {
                self.refresh(uri).await;
            }
        }
        changed
    }

    /// Ask the client for `jux.sourceRoots` (the pull half of the protocol).
    /// `None` when the client cannot answer (it does not support
    /// `workspace/configuration`, or has no such section).
    async fn pull_roots(&self) -> Option<SourceRoots> {
        let items = vec![ConfigurationItem { scope_uri: None, section: Some(ROOTS_SECTION.to_string()) }];
        let values = self.client.configuration(items).await.ok()?;
        values.first().and_then(SourceRoots::from_settings)
    }

    /// A snapshot of the document at `uri` for `completionItem/resolve`.
    fn snapshot(&self, uri: &Url) -> Option<std::sync::Arc<DocSnapshot>> {
        let doc = self.docs.get(uri)?;
        Some(std::sync::Arc::new(DocSnapshot {
            text: doc.rope.to_string(),
            symbols: doc.symbols.clone(),
            source_paths: doc.source_paths.clone(),
        }))
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Heartbeat: watch the IDE's process id and exit if it dies. The LSP
        // spec recommends this: if the parent goes away (crash, kill, or a
        // dropped connection that didn't EOF our stdin), the server must not
        // linger. Combined with the stdin-EOF exit in `main`, this guarantees
        // `juxc-lsp` stops whenever the IDE does.
        if let Some(pid) = params.process_id {
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(std::time::Duration::from_secs(3));
                loop {
                    tick.tick().await;
                    if !parent_alive(pid) {
                        std::process::exit(0);
                    }
                }
            });
        }

        // Record the project root (for workspace indexing): prefer the first
        // workspace folder, then the legacy `rootUri`.
        let root = params
            .workspace_folders
            .as_ref()
            .and_then(|folders| folders.first())
            .map(|f| f.uri.clone())
            .or(params.root_uri.clone())
            .and_then(|uri| uri.to_file_path().ok());
        if let (Some(root), Ok(mut ws)) = (root, self.workspace.write()) {
            ws.root = Some(root);
        }

        // §L.8: UTF-8 columns when the client offers them, UTF-16 otherwise.
        let offered = params
            .capabilities
            .general
            .as_ref()
            .and_then(|g| g.position_encodings.as_deref());
        let enc = negotiate(offered);
        if let Ok(mut e) = self.encoding.write() {
            *e = enc;
        }

        // Renaming a type in `Name.jux` moves the file, when the client can.
        let edit_caps = params.capabilities.workspace.as_ref().and_then(|w| w.workspace_edit.as_ref());
        let renames_files = edit_caps.is_some_and(|w| {
            w.document_changes == Some(true)
                && w
                    .resource_operations
                    .as_ref()
                    .is_some_and(|ops| ops.contains(&ResourceOperationKind::Rename))
        });
        self.file_renames.store(renames_files, Ordering::Relaxed);

        // The source roots the client passes up front (§I.4).
        if let Some(roots) = params.initialization_options.as_ref().and_then(SourceRoots::from_settings) {
            if let Ok(mut r) = self.roots.write() {
                *r = roots;
            }
        }

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "juxc-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: crate::capabilities::server_capabilities(enc),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "juxc-lsp ready")
            .await;
        // No roots in the initialization options: ask for them.
        if self.roots().is_empty() {
            if let Some(roots) = self.pull_roots().await {
                if let Ok(mut r) = self.roots.write() {
                    *r = roots;
                }
            }
        }
        // Generate/refresh foreign-crate stubs for the project's `rust.*` deps
        // BEFORE indexing, since the index scans `.jux-stubs/`. This makes bound
        // Rust crates autocomplete in Jux syntax on first open, no build needed.
        self.ensure_crate_stubs().await;
        self.reindex().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        self.docs.insert(
            doc.uri.clone(),
            Document::new(Rope::from_str(&doc.text), doc.version, self.encoding()),
        );
        self.refresh(doc.uri).await;
        // A newly opened file may belong to a not-yet-indexed module.
        self.reindex().await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // Saving `jux.toml` may add/remove a `rust.*` dependency, so regenerate
        // the project's crate stubs before re-indexing: a freshly added crate
        // then autocompletes without restarting the server. (`ensure_crate_stubs`
        // is cache-keyed, so an unchanged manifest is a cheap no-op.)
        let is_manifest = params
            .text_document
            .uri
            .to_file_path()
            .ok()
            .and_then(|p| p.file_name().map(|n| n == "jux.toml"))
            .unwrap_or(false);
        if is_manifest {
            self.ensure_crate_stubs().await;
        }
        // Saving any file may add/rename types or members anywhere: refresh the
        // cross-module index.
        self.reindex().await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let enc = self.encoding();
        match self.docs.get_mut(&uri) {
            Some(mut doc) => {
                crate::sync::apply_changes(&mut doc.rope, &params.content_changes, enc);
                doc.version = params.text_document.version;
            }
            None => {
                // A change for a buffer we never saw open: only a whole-text
                // change can be adopted.
                let Some(full) = params.content_changes.iter().rev().find(|c| c.range.is_none()) else {
                    return;
                };
                self.docs.insert(
                    uri.clone(),
                    Document::new(Rope::from_str(&full.text), params.text_document.version, enc),
                );
            }
        }
        self.refresh(uri).await;
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.docs.remove(&params.text_document.uri);
        // Clear diagnostics for the now-closed file.
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn did_change_configuration(&self, params: DidChangeConfigurationParams) {
        // Push form: the settings carry the roots. Pull form (null settings,
        // or settings without them): ask.
        let roots = match SourceRoots::from_settings(&params.settings) {
            Some(r) => Some(r),
            None if params.settings == Value::Null => self.pull_roots().await,
            None => None,
        };
        if let Some(roots) = roots {
            self.set_roots(roots).await;
        }
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        Ok(self.docs.get(&uri).and_then(|doc| crate::hover::hover(&doc, &uri, pos)))
    }

    async fn goto_definition(&self, params: GotoDefinitionParams) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        Ok(self
            .docs
            .get(&uri)
            .and_then(|doc| crate::definition::goto_definition(&doc, &uri, pos)))
    }

    async fn document_symbol(&self, params: DocumentSymbolParams) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        // Re-parse the open buffer (cheap; lex + parse only) to get its AST: the
        // analysis keeps the merged symbol table but not the per-file AST.
        let text = doc.rope.to_string();
        let path = uri.to_file_path().unwrap_or_else(|_| PathBuf::from("buffer.jux"));
        let source = juxc_source::SourceFile::new(path, text);
        let lexed = juxc_lex::lex(&source);
        let parsed = juxc_parse::parse(&lexed.tokens);
        let symbols: Vec<DocumentSymbol> = parsed
            .ast
            .items
            .iter()
            .filter_map(|item| crate::symbols::top_level_document_symbol(item, &doc.rope, doc.enc))
            .collect();
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(Some(CompletionResponse::Array(Vec::new())));
        };
        let offset = doc.offset_at(pos);
        let items = match self.workspace.read() {
            Ok(ws) => build_completions(&doc, &ws, &uri, offset),
            Err(_) => build_completions(&doc, &Workspace::default(), &uri, offset),
        };
        Ok(Some(CompletionResponse::Array(items)))
    }

    async fn completion_resolve(&self, item: CompletionItem) -> Result<CompletionItem> {
        Ok(resolve_item(item, &|uri| self.snapshot(uri)))
    }

    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        Ok(self.docs.get(&uri).and_then(|doc| crate::calls::signature_help(&doc, pos)))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        let Ok(ws) = self.workspace.read() else {
            return Ok(None);
        };
        Ok(Some(crate::code_action::code_actions(&doc, &ws, &uri, &params.context.diagnostics)))
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let include = params.context.include_declaration;
        Ok(self
            .docs
            .get(&uri)
            .map(|doc| crate::references::references(&doc, &uri, pos, include)))
    }

    async fn prepare_rename(&self, params: TextDocumentPositionParams) -> Result<Option<PrepareRenameResponse>> {
        let Some(doc) = self.docs.get(&params.text_document.uri) else {
            return Ok(None);
        };
        crate::references::prepare_rename(&doc, params.position).map_err(RpcError::invalid_params)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        let file_renames = self.file_renames.load(Ordering::Relaxed);
        crate::references::rename(&doc, &uri, pos, &params.new_name, file_renames)
            .map_err(RpcError::invalid_params)
    }

    async fn semantic_tokens_full(&self, params: SemanticTokensParams) -> Result<Option<SemanticTokensResult>> {
        Ok(self
            .docs
            .get(&params.text_document.uri)
            .map(|doc| SemanticTokensResult::Tokens(crate::semtok::semantic_tokens(&doc, None))))
    }

    async fn semantic_tokens_range(
        &self,
        params: SemanticTokensRangeParams,
    ) -> Result<Option<SemanticTokensRangeResult>> {
        Ok(self.docs.get(&params.text_document.uri).map(|doc| {
            SemanticTokensRangeResult::Tokens(crate::semtok::semantic_tokens(&doc, Some(params.range)))
        }))
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        Ok(self
            .docs
            .get(&params.text_document.uri)
            .map(|doc| crate::inlay::inlay_hints(&doc, params.range)))
    }

    async fn symbol(&self, params: WorkspaceSymbolParams) -> Result<Option<Vec<SymbolInformation>>> {
        let enc = self.encoding();
        // The last whole-project index, else any open document's analysis.
        if let Ok(ws) = self.workspace.read() {
            if let Some(symbols) = ws.symbols.as_deref() {
                let src = crate::symbols::SymbolSource {
                    symbols,
                    paths: &ws.source_paths,
                    texts: &ws.source_texts,
                };
                return Ok(Some(crate::symbols::workspace_symbols(&src, &params.query, enc)));
            }
        }
        let Some(doc) = self.docs.iter().next() else {
            return Ok(Some(Vec::new()));
        };
        let src = crate::symbols::SymbolSource {
            symbols: &doc.symbols,
            paths: &doc.source_paths,
            texts: &doc.source_texts,
        };
        Ok(Some(crate::symbols::workspace_symbols(&src, &params.query, enc)))
    }
}
