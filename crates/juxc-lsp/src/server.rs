//! The `LanguageServer` implementation — request routing and the document
//! store.
//!
//! Phase-1/2 capabilities (§L.5): full-document sync, push diagnostics, hover
//! (expression type under the cursor), and completion (keywords + in-scope
//! type names). Goto-definition, references, and rename are advertised off
//! until the AST cross-reference index lands.

use std::collections::HashMap;
use std::sync::RwLock;

use dashmap::DashMap;
use ropey::Rope;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::analysis::{analyze_single, analyze_workspace};
use crate::doc::Document;
use crate::position::{position_to_offset, span_to_range};
use crate::workspace::Workspace;

/// Jux keywords offered by completion. Java-shaped by design — this is the set
/// an editor colours and suggests. Kept in sync with the lexer's keyword
/// table (`JUX-GRAMMAR-ADDENDUM.md` §A; the lexer is the normative source).
/// Built-in type names offered by completion in every context (a type can name
/// a field, a return type, a local, a cast, …). Coloured as types in the editor.
const PRIMITIVES: &[&str] = &[
    "bool", "char", "byte", "short", "int", "long", "float", "double", "ubyte", "ushort", "uint",
    "ulong", "never", "String", "void",
];

/// Literal constants — only meaningful in expressions (statement context).
const CONSTANTS: &[&str] = &["true", "false", "null"];

/// Keywords valid at the **top level** of a file (package / imports / type
/// declarations and their modifiers).
const TOPLEVEL_KEYWORDS: &[&str] = &[
    "package", "import", "public", "private", "protected", "internal", "abstract", "final",
    "sealed", "static", "const", "class", "interface", "enum", "struct", "record", "annotation",
    "type", "async", "native", "operator", "permits", "extends", "implements",
];

/// Keywords valid inside a **type body** (member declarations + modifiers). No
/// statement keywords, no `print`.
const MEMBER_KEYWORDS: &[&str] = &[
    "public", "private", "protected", "internal", "static", "final", "abstract", "const", "async",
    "operator", "default", "throws", "extends", "implements", "permits",
];

/// Keywords valid inside a **function / method body** (statements & expressions).
const STATEMENT_KEYWORDS: &[&str] = &[
    "var", "return", "if", "else", "for", "while", "do", "switch", "case", "default", "break",
    "continue", "new", "this", "super", "throw", "try", "catch", "finally", "await", "yield", "is",
    "as", "in", "sizeof", "unsafe", "move", "drop",
];

/// Snippets offered at the top level / in a type body (declaration templates).
const DECL_SNIPPETS: &[(&str, &str)] = &[
    ("class", "public class ${1:Name} {\n    $0\n}"),
    ("interface", "public interface ${1:Name} {\n    $0\n}"),
    ("enum", "public enum ${1:Name} {\n    $0\n}"),
    ("struct", "public struct ${1:Name} {\n    $0\n}"),
    ("record", "public record ${1:Name}(${2}) {\n    $0\n}"),
    ("main", "public void main() {\n    $0\n}"),
];

/// Snippets offered inside a function / method body (statement templates).
const STMT_SNIPPETS: &[(&str, &str)] = &[
    ("print", "print($0);"),
    ("if", "if ($1) {\n    $0\n}"),
    ("ifelse", "if ($1) {\n    $2\n} else {\n    $0\n}"),
    ("for", "for (int ${1:i} = 0; $1 < ${2:n}; $1++) {\n    $0\n}"),
    ("while", "while ($1) {\n    $0\n}"),
    ("switch", "switch ($1) {\n    $0\n}"),
    ("return", "return $0;"),
];

/// Where the cursor sits, structurally — drives which completions are offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CtxKind {
    /// Outside any braces — package/imports/type declarations.
    TopLevel,
    /// Inside a `class`/`interface`/`enum`/… body — member declarations.
    TypeBody,
    /// Inside a function/method body or nested block — statements.
    Statement,
}

/// Classify the cursor context from the document text **before** the cursor.
///
/// Lightweight and PSI-free: it scans the prefix (skipping comments, strings,
/// and char literals) tracking a stack of brace "headers". The header that
/// precedes each `{` tells us whether that block is a type body (its header
/// names a `class`/`interface`/…) or a function/statement block. The innermost
/// open block decides the context; no open block means top level.
fn analyze_context(prefix: &str) -> CtxKind {
    // Each stack entry: was the opening brace a type body?
    let mut stack: Vec<bool> = Vec::new();
    let mut seg = String::new();
    let mut chars = prefix.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '/' => match chars.peek() {
                Some('/') => {
                    for c2 in chars.by_ref() {
                        if c2 == '\n' {
                            break;
                        }
                    }
                    seg.clear();
                }
                Some('*') => {
                    chars.next();
                    let mut prev = ' ';
                    for c2 in chars.by_ref() {
                        if prev == '*' && c2 == '/' {
                            break;
                        }
                        prev = c2;
                    }
                    seg.push(' ');
                }
                _ => seg.push(c),
            },
            '"' => {
                while let Some(c2) = chars.next() {
                    if c2 == '\\' {
                        chars.next();
                    } else if c2 == '"' {
                        break;
                    }
                }
                seg.push('"');
            }
            '\'' => {
                while let Some(c2) = chars.next() {
                    if c2 == '\\' {
                        chars.next();
                    } else if c2 == '\'' {
                        break;
                    }
                }
                seg.push('\'');
            }
            '{' => {
                stack.push(header_is_type(&seg));
                seg.clear();
            }
            '}' => {
                stack.pop();
                seg.clear();
            }
            ';' => seg.clear(),
            _ => seg.push(c),
        }
    }

    match stack.last() {
        None => CtxKind::TopLevel,
        Some(true) => CtxKind::TypeBody,
        Some(false) => CtxKind::Statement,
    }
}

/// True if a brace's preceding header declares a type (so the block is a type
/// body), e.g. `public class Foo<T>`. A function header like
/// `public void main()` has no type keyword and is treated as a statement block.
fn header_is_type(seg: &str) -> bool {
    const TYPE_KW: &[&str] = &["class", "interface", "enum", "struct", "record", "annotation"];
    seg.split(|c: char| !c.is_alphanumeric() && c != '_')
        .any(|w| TYPE_KW.contains(&w))
}

/// Is the process with id `pid` still alive? Used by the parent-process
/// heartbeat. Dependency-free: on Windows it queries the process exit code via
/// `kernel32`; elsewhere it conservatively returns `true` (those platforms
/// rely on the stdin-EOF exit path instead).
#[cfg(windows)]
fn parent_alive(pid: u32) -> bool {
    use std::os::raw::c_void;
    // Minimal kernel32 bindings — avoids pulling in a winapi crate.
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
    /// Project-wide index (root + all module type names) for completion.
    workspace: RwLock<Workspace>,
    /// URIs we last published a non-empty diagnostic set for. A workspace
    /// check reports diagnostics for *several* files at once; when a later
    /// check finds a file clean, we must publish an empty list to clear it.
    /// We track the previously-dirty set so we can clear exactly those URIs
    /// the new pass didn't re-report.
    published: DashMap<Url, ()>,
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
        }
    }

    /// Re-scan every `.jux` file in the project and refresh the workspace
    /// type-name index used by completion. Runs the (heavy) analysis on a
    /// blocking thread; cheap to call on open/save. No-op until a root is set.
    async fn reindex(&self) {
        let root = match self.workspace.read() {
            Ok(ws) => ws.root.clone(),
            Err(_) => return,
        };
        let Some(root) = root else { return };

        // Snapshot live editor text for open buffers so the index reflects
        // unsaved edits.
        let mut overrides: HashMap<std::path::PathBuf, String> = HashMap::new();
        for entry in self.docs.iter() {
            if let Ok(path) = entry.key().to_file_path() {
                overrides.insert(path, entry.value().rope.to_string());
            }
        }

        let index =
            tokio::task::spawn_blocking(move || crate::workspace::index_workspace(&root, &overrides))
                .await
                .unwrap_or_default();

        if let Ok(mut ws) = self.workspace.write() {
            ws.type_names = index.type_names;
            ws.member_names = index.member_names;
        }
    }

    /// Re-analyse `text` for `uri`, cache the result, and publish diagnostics.
    /// Called on open and on every change (full-document sync).
    ///
    /// The analysis (lex → parse → resolve → tycheck over the whole stdlib + the
    /// document) is CPU-bound and runs on **every** keystroke, so we hand it to
    /// `spawn_blocking`. That keeps it off the async worker threads and stops a
    /// burst of edits from stalling the server (which the editor perceives as a
    /// UI freeze while it waits for diagnostics/completion).
    async fn refresh(&self, uri: Url, text: &str, version: i32) {
        let rope = Rope::from_str(text);

        // Determine the workspace root captured at `initialize`. With a root
        // we check the WHOLE workspace together (so cross-file imported types
        // carry their real method tables and the false `[E0413]` is gone);
        // without one we fall back to single-file behavior.
        let root = self.workspace.read().ok().and_then(|ws| ws.root.clone());

        // Clone the (cheap, copy-on-write) rope + uri into the blocking task.
        // The front end is CPU-bound and runs on every keystroke, so it goes
        // on a blocking thread to keep the async workers responsive.
        let task_uri = uri.clone();
        let task_rope = rope.clone();
        let analysis = match tokio::task::spawn_blocking(move || match root {
            Some(root) => analyze_workspace(&root, &task_uri, &task_rope),
            None => analyze_single(&task_uri, &task_rope),
        })
        .await
        {
            Ok(a) => a,
            Err(_) => return, // the blocking task panicked; skip this revision
        };

        let doc = Document {
            rope,
            version,
            expr_types: analysis.expr_types,
            type_names: analysis.type_names,
        };
        self.docs.insert(uri.clone(), doc);

        // Publish diagnostics PER FILE. A workspace check can surface errors
        // in files other than the open one, so we publish each file's group
        // under its own URI. We also clear any file we previously reported
        // dirty that this pass found clean (or didn't analyse): publishing an
        // empty list is how LSP clears.
        let mut now_dirty: Vec<Url> = Vec::new();
        for (file_uri, diags) in analysis.diagnostics_by_uri {
            if !diags.is_empty() {
                now_dirty.push(file_uri.clone());
            }
            // Version only applies to the open document; other files get None.
            let ver = if file_uri == uri { Some(version) } else { None };
            self.client
                .publish_diagnostics(file_uri, diags, ver)
                .await;
        }

        // Clear files that were dirty before but aren't in this pass's output.
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
        // Record the new dirty set.
        for u in now_dirty {
            self.published.insert(u, ());
        }
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, params: InitializeParams) -> Result<InitializeResult> {
        // Heartbeat: watch the IDE's process id and exit if it dies. The LSP
        // spec recommends this — if the parent goes away (crash, kill, or a
        // dropped connection that didn't EOF our stdin), the server must not
        // linger. Combined with the stdin-EOF exit in `main`, this guarantees
        // `juxc-lsp` stops whenever the IDE does.
        if let Some(pid) = params.process_id {
            let pid = pid as u32;
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
            .or(params.root_uri)
            .and_then(|uri| uri.to_file_path().ok());
        if let (Some(root), Ok(mut ws)) = (root, self.workspace.write()) {
            ws.root = Some(root);
        }

        Ok(InitializeResult {
            server_info: Some(ServerInfo {
                name: "juxc-lsp".to_string(),
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
            }),
            capabilities: ServerCapabilities {
                // Full-document sync keeps the skeleton simple: each change
                // ships the whole buffer. Incremental sync is a later
                // optimization (§L.5).
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                completion_provider: Some(CompletionOptions {
                    // `.` member access, `:` path, `@` annotation (§L.5).
                    trigger_characters: Some(vec![".".into(), ":".into(), "@".into()]),
                    ..Default::default()
                }),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "juxc-lsp ready")
            .await;
        // Build the initial project-wide index (all classes/types/members).
        self.reindex().await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = params.text_document;
        self.refresh(doc.uri, &doc.text, doc.version).await;
        // A newly opened file may belong to a not-yet-indexed module.
        self.reindex().await;
    }

    async fn did_save(&self, params: DidSaveTextDocumentParams) {
        // Saving a file may add/rename types or members anywhere — refresh the
        // cross-module index. (Per-keystroke changes don't trigger this; the
        // open buffer's own names come from its live single-file analysis.)
        let _ = params;
        self.reindex().await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        // Full sync: the last content change carries the entire new text.
        if let Some(change) = params.content_changes.into_iter().last() {
            self.refresh(
                params.text_document.uri,
                &change.text,
                params.text_document.version,
            )
            .await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.docs.remove(&params.text_document.uri);
        // Clear diagnostics for the now-closed file.
        self.client
            .publish_diagnostics(params.text_document.uri, Vec::new(), None)
            .await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        let offset = position_to_offset(&doc.rope, pos);
        let Some((span, ty)) = doc.type_at(offset) else {
            return Ok(None);
        };
        // Render the inferred type as a Jux code block so editors syntax-
        // colour it.
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("```jux\n{ty}\n```"),
            }),
            range: Some(span_to_range(&doc.rope, span)),
        }))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let pos = params.text_document_position.position;

        let Some(doc) = self.docs.get(&uri) else {
            return Ok(Some(CompletionResponse::Array(Vec::new())));
        };

        // Classify the cursor context from the text before it, so we only
        // offer relevant completions (e.g. `print` / statements only inside a
        // function body; `class` / modifiers only at the top level).
        let offset = position_to_offset(&doc.rope, pos);
        let prefix: String = doc.rope.slice(..doc.rope.byte_to_char(offset.min(doc.rope.len_bytes()))).to_string();
        let ctx = analyze_context(&prefix);

        let mut items: Vec<CompletionItem> = Vec::new();

        // Snippets (sorted to the top) — declaration templates at top level /
        // type body, statement templates inside a function body.
        let snippets: &[(&str, &str)] = match ctx {
            CtxKind::Statement => STMT_SNIPPETS,
            CtxKind::TopLevel | CtxKind::TypeBody => DECL_SNIPPETS,
        };
        for (label, body) in snippets {
            items.push(CompletionItem {
                label: (*label).to_string(),
                kind: Some(CompletionItemKind::SNIPPET),
                detail: Some("snippet".to_string()),
                insert_text: Some((*body).to_string()),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                sort_text: Some(format!("0_{label}")),
                ..Default::default()
            });
        }

        // Keywords for this context.
        let keywords: &[&str] = match ctx {
            CtxKind::TopLevel => TOPLEVEL_KEYWORDS,
            CtxKind::TypeBody => MEMBER_KEYWORDS,
            CtxKind::Statement => STATEMENT_KEYWORDS,
        };
        for kw in keywords {
            items.push(CompletionItem {
                label: (*kw).to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                sort_text: Some(format!("3_{kw}")),
                ..Default::default()
            });
        }

        // Built-in types — useful in every context.
        for ty in PRIMITIVES {
            items.push(CompletionItem {
                label: (*ty).to_string(),
                kind: Some(CompletionItemKind::STRUCT),
                detail: Some("built-in type".to_string()),
                sort_text: Some(format!("2_{ty}")),
                ..Default::default()
            });
        }

        // Literal constants — expressions only.
        if ctx == CtxKind::Statement {
            for c in CONSTANTS {
                items.push(CompletionItem {
                    label: (*c).to_string(),
                    kind: Some(CompletionItemKind::CONSTANT),
                    sort_text: Some(format!("2_{c}")),
                    ..Default::default()
                });
            }
        }

        // Track labels already added so the workspace index doesn't duplicate
        // the open file's own names.
        let mut seen: std::collections::HashSet<String> =
            items.iter().map(|i| i.label.clone()).collect();

        // In-scope type names from the open file's live analysis (fresh,
        // includes types just typed but not yet saved).
        for name in &doc.type_names {
            if seen.insert(name.clone()) {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::CLASS),
                    sort_text: Some(format!("1_{name}")),
                    ..Default::default()
                });
            }
        }

        // Project-wide index: every class/type and every function/method/field
        // from all `.jux` modules (refreshed on open/save).
        if let Ok(ws) = self.workspace.read() {
            for name in &ws.type_names {
                if seen.insert(name.clone()) {
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::CLASS),
                        detail: Some("project type".to_string()),
                        sort_text: Some(format!("1_{name}")),
                        ..Default::default()
                    });
                }
            }
            // Members only make sense in expression position (a function body).
            if ctx == CtxKind::Statement {
                for name in &ws.member_names {
                    if seen.insert(name.clone()) {
                        items.push(CompletionItem {
                            label: name.clone(),
                            kind: Some(CompletionItemKind::FUNCTION),
                            detail: Some("project member".to_string()),
                            sort_text: Some(format!("4_{name}")),
                            ..Default::default()
                        });
                    }
                }
            }
        }

        Ok(Some(CompletionResponse::Array(items)))
    }
}
