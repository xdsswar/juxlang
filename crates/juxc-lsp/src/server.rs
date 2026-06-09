//! The `LanguageServer` implementation — request routing and the document
//! store.
//!
//! Phase-1/2 capabilities (§L.5): full-document sync, push diagnostics, hover
//! (expression type under the cursor + declaration signatures), completion
//! (keywords, in-scope type names, receiver-aware members), auto-import code
//! actions, **goto-definition** for type / function / const / alias names
//! (resolved through `SymbolTable::decl_unit` → `source_paths`, reaching into
//! generated `rust.std` / crate `.jux.d` stubs), and **document symbols** (an
//! outline of the open file's declarations with members nested). References and
//! rename remain advertised off until the AST cross-reference index lands;
//! member-level goto-definition awaits per-member source spans.

use std::collections::HashMap;
use std::sync::RwLock;

use dashmap::DashMap;
use juxc_source::Span;
use ropey::Rope;
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer};

use crate::analysis::{analyze_single, analyze_workspace};
use crate::doc::Document;
use crate::intel;
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

/// An identifier found in the source text, with its byte range.
struct Word {
    /// The identifier text.
    text: String,
    /// Inclusive start byte offset.
    start: usize,
    /// Exclusive end byte offset.
    end: usize,
}

/// True for a byte that can appear in a Jux identifier.
fn is_ident_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

/// Extract the identifier whose span contains `offset` (or that ends exactly at
/// `offset`, so a cursor parked just after a name still resolves it). Returns
/// `None` when `offset` isn't inside / adjacent to an identifier. ASCII-only
/// boundary scan — adequate for Jux identifiers, which are ASCII.
fn word_at(text: &str, offset: usize) -> Option<Word> {
    let bytes = text.as_bytes();
    let len = bytes.len();
    if offset > len {
        return None;
    }
    // The cursor may sit just past the identifier's last byte; step back one
    // when the byte at `offset` isn't an identifier byte but the previous is.
    let probe = if offset < len && is_ident_byte(bytes[offset]) {
        offset
    } else if offset > 0 && is_ident_byte(bytes[offset - 1]) {
        offset - 1
    } else {
        return None;
    };
    let mut start = probe;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = probe;
    while end < len && is_ident_byte(bytes[end]) {
        end += 1;
    }
    // An identifier can't start with a digit — if it does, the cursor is on a
    // numeric literal, not a name.
    if bytes[start].is_ascii_digit() {
        return None;
    }
    Some(Word { text: text[start..end].to_string(), start, end })
}

/// The start offset of the identifier run that ends at `offset` (the partial
/// word the user is typing). Returns `offset` unchanged when the byte before
/// the cursor isn't an identifier byte (e.g. the cursor is right after a `.`).
fn ident_start_before(text: &str, offset: usize) -> usize {
    let bytes = text.as_bytes();
    let mut start = offset.min(bytes.len());
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    start
}

/// Recover a receiver expression's type when the **live** analysis couldn't —
/// the typical mid-edit case where the user has typed `obj.` (optionally with a
/// partial member name) but nothing after, so the dangling member access fails
/// to parse and the receiver is left untyped.
///
/// We build a patched buffer in which the `.`-through-caret slice is replaced
/// by a single `;`, turning `… obj.<partial>` into the complete statement
/// `… obj;`. Re-analysing that buffer types the receiver, whose expression span
/// still ends exactly at `recv_end` (the original `.`'s offset) — so the same
/// `end == recv_end` lookup the cache uses finds it. The reparse goes through
/// the *workspace* path when a root is known (so a receiver of a cross-file type
/// resolves too), else the single-file path.
///
/// Only the receiver's `Ty` is taken from the reparse; member resolution still
/// runs against the stable cached symbol table, which always carries the full
/// project + stdlib surface.
fn receiver_type_by_reparse(
    text: &str,
    recv_end: usize,
    caret: usize,
    root: Option<&std::path::Path>,
    uri: &Url,
) -> Option<juxc_tycheck::Ty> {
    // Guard the slice bounds (offsets come from byte scans, but stay defensive).
    if recv_end > caret || caret > text.len() || !text.is_char_boundary(recv_end) || !text.is_char_boundary(caret) {
        return None;
    }
    let mut patched = String::with_capacity(text.len());
    patched.push_str(&text[..recv_end]);
    patched.push(';');
    patched.push_str(&text[caret..]);

    let rope = Rope::from_str(&patched);
    let analysis = match root {
        Some(r) => analyze_workspace(r, uri, &rope),
        None => analyze_single(uri, &rope),
    };
    // The receiver expression now ends at `recv_end` (the `;` we inserted sits
    // right after it); pick the largest span ending there, as the cache does.
    analysis
        .expr_types
        .iter()
        .filter(|(s, _)| s.end as usize == recv_end)
        .max_by_key(|(s, _)| s.len())
        .map(|(_, t)| t.clone())
}

/// True when the identifier starting at `ident_start` sits in the **type
/// position of a `new <Type>` expression** — the nearest word before it
/// (skipping whitespace) is the `new` keyword. In that position completion
/// offers only type names, inserted bare, so `new X` stays `X` rather than the
/// spurious `X()` IntelliJ appends to function-shaped items.
fn is_new_context(text: &str, ident_start: usize) -> bool {
    let bytes = text.as_bytes();
    let mut end = ident_start.min(bytes.len());
    while end > 0 && (bytes[end - 1] as char).is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_ident_byte(bytes[start - 1]) {
        start -= 1;
    }
    &text[start..end] == "new"
}

/// Scanning left from `offset`, find the `(` that opens the call whose argument
/// list the caret sits inside, plus the active parameter index (the number of
/// top-level commas before the caret). Nested parentheses are balanced; a
/// top-level `;`/`{`/`}` ends the search (we're no longer inside a call).
/// Returns `None` when the caret isn't inside any call's `( … )`.
fn find_enclosing_call(text: &str, offset: usize) -> Option<(usize, u32)> {
    let bytes = text.as_bytes();
    let mut i = offset.min(bytes.len());
    let mut depth = 0i32;
    let mut commas = 0u32;
    while i > 0 {
        i -= 1;
        match bytes[i] {
            b')' => depth += 1,
            b'(' => {
                if depth == 0 {
                    return Some((i, commas));
                }
                depth -= 1;
            }
            b',' if depth == 0 => commas += 1,
            b';' | b'{' | b'}' if depth == 0 => return None,
            _ => {}
        }
    }
    None
}

/// Build one [`SignatureInformation`] for `callee` from its parameter list,
/// rendering each parameter as `Type name` (Jux syntax) so the popup reads like
/// the declaration.
fn signature_info(
    callee: &str,
    params: &[juxc_tycheck::symbol_table::ParamSig],
) -> SignatureInformation {
    let labels: Vec<String> = params
        .iter()
        .map(|p| format!("{} {}", intel::render_type(&p.ty), p.name))
        .collect();
    let label = format!("{callee}({})", labels.join(", "));
    let parameters = labels
        .into_iter()
        .map(|l| ParameterInformation {
            label: ParameterLabel::Simple(l),
            documentation: None,
        })
        .collect();
    SignatureInformation {
        label,
        documentation: None,
        parameters: Some(parameters),
        active_parameter: None,
    }
}

/// If the identifier starting at `ident_start` is the member half of a
/// `receiver.member` access, return the byte offset of the receiver's last
/// byte (i.e. the `.`'s offset) so the receiver expression's span — which ends
/// there — can be looked up. Skips ASCII whitespace between the `.` and the
/// identifier. Returns `None` when there's no preceding `.` (a plain name) or
/// the `.` is part of a number / float.
fn receiver_dot_before(text: &str, ident_start: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = ident_start;
    while i > 0 && (bytes[i - 1] as char).is_ascii_whitespace() {
        i -= 1;
    }
    if i == 0 || bytes[i - 1] != b'.' {
        return None;
    }
    let dot = i - 1;
    // `1.0` style float: a `.` preceded by a digit isn't a member access.
    if dot > 0 && bytes[dot - 1].is_ascii_digit() {
        return None;
    }
    // The receiver expression's span ends at the `.`'s offset (exclusive end).
    Some(dot)
}

/// Map a top-level declaration to a hierarchical [`DocumentSymbol`] for the
/// editor outline: the declaration itself, with its members (methods, fields,
/// variants) nested as children. `range` covers the whole declaration;
/// `selection_range` is the name. Returns `None` for an unnameable item.
fn top_level_document_symbol(item: &juxc_ast::TopLevelDecl, rope: &Rope) -> Option<DocumentSymbol> {
    use juxc_ast::TopLevelDecl as T;
    let sym = |name: &str, kind: SymbolKind, decl: Span, name_span: Span, children: Vec<DocumentSymbol>| {
        #[allow(deprecated)]
        DocumentSymbol {
            name: name.to_string(),
            detail: None,
            kind,
            tags: None,
            deprecated: None,
            range: span_to_range(rope, decl),
            selection_range: span_to_range(rope, name_span),
            children: if children.is_empty() { None } else { Some(children) },
        }
    };
    match item {
        T::Class(c) => {
            let mut kids = Vec::new();
            for f in &c.fields {
                kids.push(sym(&f.name.text, SymbolKind::FIELD, f.span, f.name.span, Vec::new()));
            }
            for m in &c.methods {
                kids.push(sym(&m.name.text, SymbolKind::METHOD, m.span, m.name.span, Vec::new()));
            }
            // A `struct`-origin class reads as a STRUCT in the outline.
            let kind = if c.is_struct { SymbolKind::STRUCT } else { SymbolKind::CLASS };
            Some(sym(&c.name.text, kind, c.span, c.name.span, kids))
        }
        T::Interface(i) => {
            let kids = i
                .methods
                .iter()
                .map(|m| sym(&m.name.text, SymbolKind::METHOD, m.span, m.name.span, Vec::new()))
                .collect();
            Some(sym(&i.name.text, SymbolKind::INTERFACE, i.span, i.name.span, kids))
        }
        T::Enum(e) => {
            let kids = e
                .variants
                .iter()
                .map(|v| sym(&v.name.text, SymbolKind::ENUM_MEMBER, v.span, v.name.span, Vec::new()))
                .collect();
            Some(sym(&e.name.text, SymbolKind::ENUM, e.span, e.name.span, kids))
        }
        T::Record(r) => Some(sym(&r.name.text, SymbolKind::STRUCT, r.span, r.name.span, Vec::new())),
        T::Function(f) => {
            Some(sym(&f.name.text, SymbolKind::FUNCTION, f.span, f.name.span, Vec::new()))
        }
        T::Const(c) => Some(sym(&c.name.text, SymbolKind::CONSTANT, c.span, c.name.span, Vec::new())),
        T::TypeAlias(a) => {
            Some(sym(&a.name.text, SymbolKind::TYPE_PARAMETER, a.span, a.name.span, Vec::new()))
        }
    }
}

/// Extract the first line of a `///` or `/** … */` doc comment immediately
/// preceding the declaration whose name starts at `name_start`.
///
/// The AST doesn't carry doc comments, so we recover them from source: scan the
/// line(s) above the declaration. We walk backwards over blank lines and the
/// modifier/keyword run on the declaration's own line, then read a contiguous
/// run of `///` lines (or a single `/** … */`). Returns the first non-empty doc
/// line, trimmed, or `None` when there's no doc comment.
fn doc_comment_before(text: &str, name_start: usize) -> Option<String> {
    // Find the start of the line the name sits on.
    let line_start = text[..name_start].rfind('\n').map(|p| p + 1).unwrap_or(0);
    // Walk upward, collecting `///` lines, until a non-doc line.
    let mut cursor = line_start;
    let mut doc_lines: Vec<String> = Vec::new();
    while cursor > 0 {
        // Previous line's range [prev_start, cursor-1) (cursor-1 is its '\n').
        let prev_nl = text[..cursor - 1].rfind('\n').map(|p| p + 1).unwrap_or(0);
        let line = text[prev_nl..cursor - 1].trim();
        if let Some(rest) = line.strip_prefix("///") {
            doc_lines.push(rest.trim().to_string());
            cursor = prev_nl;
            continue;
        }
        // A single-line block doc `/** text */`.
        if line.starts_with("/**") && line.ends_with("*/") {
            let inner = line
                .trim_start_matches("/**")
                .trim_end_matches("*/")
                .trim()
                .to_string();
            doc_lines.push(inner);
        }
        break;
    }
    // `doc_lines` is bottom-up; the first source line is last.
    doc_lines.into_iter().rev().find(|l| !l.is_empty())
}

/// True when `text` already imports `fqn` (`import a.b.C;`, possibly with extra
/// whitespace or a trailing `as` alias on the same line). Used to dedupe the
/// auto-import edit so re-running the action is a no-op.
fn already_imports(text: &str, fqn: &str) -> bool {
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("import") else { continue };
        let rest = rest.trim().trim_end_matches(';').trim();
        // Match the exact path, or `a.b.C as X`.
        if rest == fqn || rest.starts_with(&format!("{fqn} ")) {
            return true;
        }
    }
    false
}

/// Build the [`TextEdit`] that inserts `import <fqn>;` at the right place:
/// immediately after the `package …;` line when present, else at the very top
/// of the file. The edit is a zero-width insertion (start == end) of a full
/// line. Returns `None` when `text` already imports `fqn`.
/// The dotted package declared at the top of `text` (`package a.b.c;` → `a.b.c`),
/// or `None` for a package-less file. Used to suppress auto-import suggestions
/// for a type that lives in the **same** package as the file being edited — a
/// same-package type needs no import (and a file must never offer to import the
/// very type it is declaring).
fn current_package(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("package") {
            // Require a word boundary after `package`.
            if rest.starts_with(char::is_whitespace) {
                let pkg = rest.trim().trim_end_matches(';').trim();
                if !pkg.is_empty() {
                    return Some(pkg.to_string());
                }
            }
        }
        // A non-blank, non-comment line before any `package` means there is none.
        if !t.is_empty() && !t.starts_with("//") && !t.starts_with("package") {
            break;
        }
    }
    None
}

fn import_edit(rope: &Rope, fqn: &str) -> Option<TextEdit> {
    let text = rope.to_string();
    if already_imports(&text, fqn) {
        return None;
    }
    // Locate the `package …;` declaration line, if any.
    let package_line = text
        .lines()
        .position(|line| line.trim_start().starts_with("package"));

    // Decide where the import goes and whether it needs a leading blank line.
    // Convention (Java-style): keep exactly one blank line between the
    // `package` declaration and the import block.
    let (insert_line, new_text) = match package_line {
        Some(p) => {
            // Is the line immediately after the package already blank? A
            // missing next line (package is the file's last line) counts as
            // "not blank" so we still insert the separating blank.
            let next_blank = text
                .lines()
                .nth(p + 1)
                .map_or(false, |l| l.trim().is_empty());
            if next_blank {
                // The separating blank already exists — drop the import just
                // below it (joining any existing import block).
                (p + 2, format!("import {fqn};\n"))
            } else {
                // No blank yet — insert one, then the import, so we get
                // `package …;` / <blank> / `import …;`.
                (p + 1, format!("\nimport {fqn};\n"))
            }
        }
        // Package-less file: imports go at the very top, no leading blank.
        None => (0, format!("import {fqn};\n")),
    };
    let pos = Position::new(insert_line as u32, 0);
    Some(TextEdit {
        range: Range::new(pos, pos),
        new_text,
    })
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

    /// Generate (or refresh) the project's foreign-crate (`rust.*` / `c.*` /
    /// `cpp.*`) dependency stubs under `.jux-stubs/`, so the bound crates' APIs
    /// autocomplete and auto-import in Jux syntax **without** a prior `jux build`
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
        if !report.resolved.is_empty() {
            self.client
                .log_message(
                    MessageType::INFO,
                    format!("jux: indexed {} bound-crate stub(s)", report.resolved.len()),
                )
                .await;
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
            ws.type_packages = index.type_packages;
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
            symbols: analysis.symbols,
            source_paths: analysis.source_paths,
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
                // Parameter info: when the caret is inside a call's `( … )`, show
                // the callee's signature with the active parameter highlighted —
                // re-triggered on `(` and each `,` (IntelliJ-style).
                signature_help_provider: Some(SignatureHelpOptions {
                    trigger_characters: Some(vec!["(".into(), ",".into()]),
                    retrigger_characters: Some(vec![",".into()]),
                    work_done_progress_options: Default::default(),
                }),
                // Auto-import quick-fixes for unresolved-but-known types
                // (FEATURE 3). Advertised as a simple boolean provider; the
                // handler filters to the import action itself.
                code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
                // Goto-definition: jump to a type / function / const / alias
                // declaration anywhere in the workspace — including into a
                // generated `rust.std` / crate `.jux.d` stub (§L.5).
                definition_provider: Some(OneOf::Left(true)),
                // Document symbols: an outline of the open file's declarations
                // (types + their members, functions, consts) for the editor's
                // breadcrumbs / symbol picker (§L.5).
                document_symbol_provider: Some(OneOf::Left(true)),
                ..Default::default()
            },
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "juxc-lsp ready")
            .await;
        // Generate/refresh foreign-crate stubs for the project's `rust.*` deps
        // BEFORE indexing, since the index scans `.jux-stubs/`. This makes bound
        // Rust crates autocomplete in Jux syntax on first open, no build needed.
        self.ensure_crate_stubs().await;
        // Build the initial project-wide index (all classes/types/members,
        // including the just-generated crate stubs).
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
        let text = doc.rope.to_string();

        // FEATURE 1 — declaration-signature hover. If the cursor sits on an
        // identifier that resolves to a KNOWN symbol (a type name, a free
        // function, or a member reached via the receiver's inferred type),
        // render that declaration's signature in Jux syntax plus its first-line
        // doc comment. Falls back to the expr-type hover below otherwise.
        if let Some(word) = word_at(&text, offset) {
            let resolved = if let Some(recv_end) = receiver_dot_before(&text, word.start) {
                // `recv.member` — resolve `recv`'s type, then look up the member.
                doc.type_ending_at(recv_end)
                    .and_then(|ty| intel::resolve_member(&doc.symbols, ty, &word.text))
            } else {
                None
            };
            // Plain identifier: try a type name, then a free function.
            let resolved = resolved
                .or_else(|| intel::resolve_type(&doc.symbols, &word.text))
                .or_else(|| intel::resolve_function(&doc.symbols, &word.text));

            if let Some(resolved) = resolved {
                let mut value = format!("```jux\n{}\n```", resolved.signature());
                // Doc comment: prefer the one at the **declaration** site — for a
                // type / function name we can locate the declaring unit (even a
                // generated `rust.std` stub) via `definition_of` and read its
                // `/** … */` there. Falls back to the usage-site scan (members,
                // or when the declaration can't be located).
                let decl_doc = doc.symbols.definition_of(&word.text).and_then(|(unit, span)| {
                    let path = doc.source_paths.get(unit)?;
                    let same_as_open = Url::from_file_path(path).ok().as_ref() == Some(&uri);
                    let decl_text = if same_as_open {
                        text.clone()
                    } else {
                        std::fs::read_to_string(path).ok()?
                    };
                    doc_comment_before(&decl_text, span.start as usize)
                });
                if let Some(doc_line) = decl_doc.or_else(|| doc_comment_before(&text, word.start)) {
                    value.push_str("\n\n");
                    value.push_str(&doc_line);
                }
                return Ok(Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value,
                    }),
                    range: Some(span_to_range(
                        &doc.rope,
                        Span::new(word.start as u32, word.end as u32),
                    )),
                }));
            }
        }

        // Fallback: the inferred type at the cursor, as a Jux code block.
        let Some((span, ty)) = doc.type_at(offset) else {
            return Ok(None);
        };
        Ok(Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("```jux\n{ty}\n```"),
            }),
            range: Some(span_to_range(&doc.rope, span)),
        }))
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        let offset = position_to_offset(&doc.rope, pos);
        let text = doc.rope.to_string();

        // Resolve a plain identifier — a type / function / const / alias name —
        // to its declaration. (Member-level goto, `recv.member`, needs per-member
        // source spans that aren't tracked yet; the receiver's type still
        // resolves through hover/completion.)
        let Some(word) = word_at(&text, offset) else {
            return Ok(None);
        };
        let Some((unit_idx, span)) = doc.symbols.definition_of(&word.text) else {
            return Ok(None);
        };
        let Some(path) = doc.source_paths.get(unit_idx) else {
            return Ok(None);
        };
        let Ok(target_uri) = Url::from_file_path(path) else {
            // Synthetic stdlib paths aren't real files — nothing to open.
            return Ok(None);
        };
        // Convert the declaring unit's byte span to a line/col range using that
        // file's text: the live rope when it's the open document, else the
        // on-disk content the analysis read.
        let target_rope = if target_uri == uri {
            doc.rope.clone()
        } else {
            match std::fs::read_to_string(path) {
                Ok(t) => Rope::from_str(&t),
                Err(_) => return Ok(None),
            }
        };
        let range = span_to_range(&target_rope, span);
        Ok(Some(GotoDefinitionResponse::Scalar(Location {
            uri: target_uri,
            range,
        })))
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        // Re-parse the open buffer (cheap; lex+parse only) to get its AST — the
        // analysis pass keeps the merged symbol table but not the per-file AST.
        let text = doc.rope.to_string();
        let path = uri.to_file_path().unwrap_or_else(|_| std::path::PathBuf::from("buffer.jux"));
        let source = juxc_source::SourceFile::new(path, text);
        let lexed = juxc_lex::lex(&source);
        let parsed = juxc_parse::parse(&lexed.tokens);
        let symbols: Vec<DocumentSymbol> = parsed
            .ast
            .items
            .iter()
            .filter_map(|item| top_level_document_symbol(item, &doc.rope))
            .collect();
        Ok(Some(DocumentSymbolResponse::Nested(symbols)))
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

        // FEATURE 2 — receiver-aware member completion. When the cursor sits in
        // a `<expr>.` context, resolve `<expr>`'s inferred type and offer ONLY
        // that type's methods + fields (walking the extends/implements chain).
        // The cursor's offset is just past the `.` (plus any partial member
        // name already typed); `member_start` is where that partial name began,
        // which is exactly where `receiver_dot_before` looks for the `.`.
        let text = doc.rope.to_string();
        let member_start = ident_start_before(&text, offset);
        if let Some(recv_end) = receiver_dot_before(&text, member_start) {
            // Resolve the receiver's type. The cached analysis is tried first;
            // if it has no type ending at the `.` — the common mid-edit case,
            // where a dangling `obj.` fails to parse and leaves the receiver
            // untyped — re-analyse a patched buffer so members still appear
            // while the user is typing.
            let root = self.workspace.read().ok().and_then(|ws| ws.root.clone());
            let recv_ty = doc.type_ending_at(recv_end).cloned().or_else(|| {
                receiver_type_by_reparse(&text, recv_end, offset, root.as_deref(), &uri)
            });
            if let Some(ty) = &recv_ty {
                let members = intel::members_of(&doc.symbols, ty);
                let items: Vec<CompletionItem> = members
                    .into_iter()
                    .map(|m| {
                        if m.is_method {
                            CompletionItem {
                                label: format!("{}()", m.name),
                                kind: Some(CompletionItemKind::METHOD),
                                detail: Some(m.detail),
                                insert_text: Some(format!("{}()", m.name)),
                                sort_text: Some(format!("0_{}", m.name)),
                                ..Default::default()
                            }
                        } else {
                            CompletionItem {
                                label: m.name.clone(),
                                kind: Some(CompletionItemKind::FIELD),
                                detail: Some(m.detail),
                                sort_text: Some(format!("0_{}", m.name)),
                                ..Default::default()
                            }
                        }
                    })
                    .collect();
                // Return ONLY the receiver's members — no globals/keywords
                // leak into a member-access completion.
                return Ok(Some(CompletionResponse::Array(items)));
            }
            // A member-access context whose receiver type we couldn't resolve.
            // Returning the statement snippet/keyword bag here is exactly the
            // noise the user must scroll past (`for`/`if`/`print` after `obj.`),
            // so return an empty list instead.
            return Ok(Some(CompletionResponse::Array(Vec::new())));
        }

        let mut items: Vec<CompletionItem> = Vec::new();

        // `new <Type>` position: offer ONLY type names, inserted bare (so `new X`
        // stays `X`, never `X()`). Suppresses snippets, keywords, primitives, and
        // the function/member bag — the function items are what IntelliJ auto-
        // appends `()` to, which is the spurious `X()` after `new`.
        let type_only = is_new_context(&text, member_start);

        // Snippets (sorted to the top) — declaration templates at top level /
        // type body, statement templates inside a function body.
        let snippets: &[(&str, &str)] = if type_only {
            &[]
        } else {
            match ctx {
                CtxKind::Statement => STMT_SNIPPETS,
                CtxKind::TopLevel | CtxKind::TypeBody => DECL_SNIPPETS,
            }
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
        let keywords: &[&str] = if type_only {
            &[]
        } else {
            match ctx {
                CtxKind::TopLevel => TOPLEVEL_KEYWORDS,
                CtxKind::TypeBody => MEMBER_KEYWORDS,
                CtxKind::Statement => STATEMENT_KEYWORDS,
            }
        };
        for kw in keywords {
            items.push(CompletionItem {
                label: (*kw).to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                sort_text: Some(format!("3_{kw}")),
                ..Default::default()
            });
        }

        // Built-in types — useful in every context except after `new` (there's
        // no `new int()`).
        if !type_only {
            for ty in PRIMITIVES {
                items.push(CompletionItem {
                    label: (*ty).to_string(),
                    kind: Some(CompletionItemKind::STRUCT),
                    detail: Some("built-in type".to_string()),
                    sort_text: Some(format!("2_{ty}")),
                    ..Default::default()
                });
            }
        }

        // Literal constants — expressions only.
        if !type_only && ctx == CtxKind::Statement {
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
                    // FEATURE 3 — auto-import on accept. If this project type
                    // lives in a package not yet imported by the open file,
                    // attach the `import pkg.Name;` edit as `additionalTextEdits`
                    // so accepting the completion also inserts the import. When
                    // the bare name has exactly one declaring package we can pick
                    // it unambiguously; ambiguous names are left to the explicit
                    // code action.
                    // Don't auto-import a type that lives in THIS file's own
                    // package — it's already in scope (and a file must never
                    // import the type it is itself declaring).
                    let cur_pkg = current_package(&text);
                    let import_edit = ws
                        .type_packages
                        .get(name)
                        .map(|pkgs| {
                            pkgs.iter()
                                .filter(|p| Some(p.as_str()) != cur_pkg.as_deref())
                                .collect::<Vec<_>>()
                        })
                        .filter(|pkgs| pkgs.len() == 1)
                        .and_then(|pkgs| {
                            let fqn = format!("{}.{name}", pkgs[0]);
                            import_edit(&doc.rope, &fqn)
                        });
                    let detail = match &import_edit {
                        Some(_) => Some(format!("project type — auto-imports {name}")),
                        None => Some("project type".to_string()),
                    };
                    items.push(CompletionItem {
                        label: name.clone(),
                        kind: Some(CompletionItemKind::CLASS),
                        detail,
                        sort_text: Some(format!("1_{name}")),
                        additional_text_edits: import_edit.map(|e| vec![e]),
                        ..Default::default()
                    });
                }
            }
            // Members only make sense in expression position (a function body),
            // and never in a `new <Type>` position.
            if !type_only && ctx == CtxKind::Statement {
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

    /// Parameter info (`textDocument/signatureHelp`). When the caret is inside a
    /// call's argument list, resolve the callee — a free function, a `new X`
    /// constructor, or a `recv.method` — and return its signature(s) with the
    /// active parameter (the count of top-level commas before the caret)
    /// highlighted, exactly like the IntelliJ Java popup.
    async fn signature_help(&self, params: SignatureHelpParams) -> Result<Option<SignatureHelp>> {
        let uri = params.text_document_position_params.text_document.uri;
        let pos = params.text_document_position_params.position;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        let offset = position_to_offset(&doc.rope, pos);
        let text = doc.rope.to_string();

        // Find the `(` that opens the call the caret sits inside, and how many
        // arguments precede the caret.
        let Some((open_paren, active_param)) = find_enclosing_call(&text, offset) else {
            return Ok(None);
        };

        // The callee identifier ends just before the `(` (skipping whitespace).
        let bytes = text.as_bytes();
        let mut name_end = open_paren;
        while name_end > 0 && (bytes[name_end - 1] as char).is_ascii_whitespace() {
            name_end -= 1;
        }
        let name_start = ident_start_before(&text, name_end);
        let callee = &text[name_start..name_end];
        if callee.is_empty() {
            return Ok(None);
        }

        // Classify the call shape from what precedes the callee name.
        let signatures: Vec<SignatureInformation> = if is_new_context(&text, name_start) {
            // `new X(…)` — every constructor of class `X`.
            match intel::resolve_type(&doc.symbols, callee) {
                Some(intel::Resolved::Class(_, sig)) if !sig.constructors.is_empty() => sig
                    .constructors
                    .iter()
                    .map(|c| signature_info(callee, &c.params))
                    .collect(),
                // A class with no explicit constructor → a single no-arg form.
                Some(intel::Resolved::Class(_, _)) => vec![signature_info(callee, &[])],
                _ => Vec::new(),
            }
        } else if let Some(dot) = receiver_dot_before(&text, name_start) {
            // `recv.method(…)` — resolve the receiver's type, then the method.
            match doc.type_ending_at(dot) {
                Some(recv_ty) => match intel::resolve_member(&doc.symbols, recv_ty, callee) {
                    Some(intel::Resolved::Method(_, sig)) => vec![signature_info(callee, &sig.params)],
                    _ => Vec::new(),
                },
                None => Vec::new(),
            }
        } else {
            // Free function call.
            match intel::resolve_function(&doc.symbols, callee) {
                Some(intel::Resolved::Function(_, sig)) => vec![signature_info(callee, &sig.params)],
                _ => Vec::new(),
            }
        };

        if signatures.is_empty() {
            return Ok(None);
        }
        Ok(Some(SignatureHelp {
            signatures,
            active_signature: Some(0),
            active_parameter: Some(active_param),
        }))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let Some(doc) = self.docs.get(&uri) else {
            return Ok(None);
        };
        let text = doc.rope.to_string();

        // FEATURE 3 — auto-import quick-fix. For every resolution diagnostic
        // (E03xx) the editor passed in `context.diagnostics`, extract the
        // identifier it points at; if that bare name is a known workspace type
        // whose package isn't yet imported, offer an `import pkg.Name;` action.
        let mut actions: Vec<CodeActionOrCommand> = Vec::new();
        let mut offered: std::collections::HashSet<String> = std::collections::HashSet::new();

        let Ok(ws) = self.workspace.read() else {
            return Ok(None);
        };

        for diag in &params.context.diagnostics {
            // Only resolution-phase diagnostics (`E03xx`) name an unresolved
            // type; skip everything else.
            let is_resolution = matches!(
                &diag.code,
                Some(NumberOrString::String(c)) if c.starts_with("E03")
            );
            if !is_resolution {
                continue;
            }
            // The identifier the diagnostic points at — take the word at the
            // diagnostic range's start.
            let start_off = position_to_offset(&doc.rope, diag.range.start);
            let Some(word) = word_at(&text, start_off) else { continue };

            let Some(pkgs) = ws.type_packages.get(&word.text) else { continue };
            let cur_pkg = current_package(&text);
            for pkg in pkgs {
                // A same-package type needs no import; never offer to import a
                // type that's in this file's own package (or that this file
                // declares).
                if Some(pkg.as_str()) == cur_pkg.as_deref() {
                    continue;
                }
                let fqn = format!("{pkg}.{}", word.text);
                if !offered.insert(fqn.clone()) {
                    continue;
                }
                let Some(edit) = import_edit(&doc.rope, &fqn) else { continue };
                let mut changes = HashMap::new();
                changes.insert(uri.clone(), vec![edit]);
                actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                    title: format!("Import `{fqn}`"),
                    kind: Some(CodeActionKind::QUICKFIX),
                    diagnostics: Some(vec![diag.clone()]),
                    edit: Some(WorkspaceEdit {
                        changes: Some(changes),
                        ..Default::default()
                    }),
                    is_preferred: Some(true),
                    ..Default::default()
                }));
            }
        }

        Ok(Some(actions))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::analyze_workspace;
    use crate::workspace::index_workspace;
    use std::fs;
    use std::path::PathBuf;

    fn temp_root(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("juxc_lsp_srv_test_{}_{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp root");
        dir
    }

    // ---- text-scanning helpers ----

    #[test]
    fn word_at_finds_identifier_and_handles_trailing_cursor() {
        let text = "var f = greet;";
        // Cursor inside `greet`.
        let w = word_at(text, 9).unwrap();
        assert_eq!(w.text, "greet");
        // Cursor just past the end of `greet` still resolves it.
        let w2 = word_at(text, 13).unwrap();
        assert_eq!(w2.text, "greet");
    }

    #[test]
    fn receiver_dot_before_detects_member_access() {
        let text = "f.greet";
        // `greet` starts at offset 2; the `.` is at offset 1.
        assert_eq!(receiver_dot_before(text, 2), Some(1));
        // A float `1.0` is not a member access.
        let f = "1.0";
        assert_eq!(receiver_dot_before(f, 2), None);
    }

    #[test]
    fn doc_comment_before_reads_first_line() {
        let text = "/// Greets someone.\npublic class Greeter {}";
        let name_start = text.find("Greeter").unwrap();
        assert_eq!(
            doc_comment_before(text, name_start).as_deref(),
            Some("Greets someone.")
        );
    }

    /// Hover doc resolution reads the doc comment from the DECLARING file (not
    /// the usage site): the same `definition_of` + `source_paths` +
    /// `doc_comment_before` pipeline the hover handler runs, here exercised
    /// cross-file so a `/** … */` on a type in another file surfaces on hover at
    /// the use site.
    #[test]
    fn hover_doc_comes_from_declaring_file() {
        let root = temp_root("hover_decl_doc");
        let lib = root.join("Lib.jux");
        let main = root.join("main.jux");
        std::fs::write(
            &lib,
            "package lib;\n/** A widget that does things. */\npublic class Widget { public int area(){ return 1; } }\n",
        )
        .unwrap();
        let main_src = "import lib.Widget;\npublic void run(){ var w = new Widget(); }\n";
        std::fs::write(&main, main_src).unwrap();

        let uri = Url::from_file_path(&main).unwrap();
        let rope = Rope::from_str(main_src);
        let analysis = analyze_workspace(&root, &uri, &rope);

        // The hover handler's declaration-doc path, run directly.
        let (unit, span) = analysis.symbols.definition_of("Widget").expect("Widget resolves");
        let path = &analysis.source_paths[unit];
        let decl_text = std::fs::read_to_string(path).unwrap();
        let doc = doc_comment_before(&decl_text, span.start as usize);
        assert_eq!(doc.as_deref(), Some("A widget that does things."));

        let _ = std::fs::remove_dir_all(&root);
    }

    /// A wrong-arity call (here a constructor missing its required argument)
    /// is type-checked by the workspace pass, tagged with the open file's index,
    /// and therefore PUBLISHED to that file's URI — i.e. it shows as a red
    /// squiggle in the editor, not only at compile time. Guards the
    /// file-tagging that routes tycheck diagnostics to the right document.
    #[test]
    fn arity_error_is_published_to_the_open_file() {
        let root = temp_root("arity_published");
        let main = root.join("main.jux");
        let src = "public class Other {\n\
                   \x20   public Other(String name){ }\n\
                   }\n\
                   public void main(){ var o = new Other(); }\n";
        std::fs::write(&main, src).unwrap();

        let uri = Url::from_file_path(&main).unwrap();
        let rope = Rope::from_str(src);
        let analysis = analyze_workspace(&root, &uri, &rope);

        let diags = analysis
            .diagnostics_by_uri
            .get(&uri)
            .expect("the open file has a diagnostics entry");
        assert!(
            diags
                .iter()
                .any(|d| matches!(&d.code, Some(NumberOrString::String(c)) if c == "E0411")),
            "expected the arity error (E0411) to be published to the open file, got: {diags:?}",
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    /// The document-symbol outline nests members under their type and covers
    /// every top-level kind.
    #[test]
    fn document_symbols_outline_nests_members() {
        let src = "package app;\n\
            public class Widget {\n\
                public int w;\n\
                public int area() { return 1; }\n\
            }\n\
            public enum Color { Red, Green }\n\
            public void main() {}\n";
        let source = juxc_source::SourceFile::new(std::path::PathBuf::from("t.jux"), src.to_string());
        let lexed = juxc_lex::lex(&source);
        let parsed = juxc_parse::parse(&lexed.tokens);
        let rope = Rope::from_str(src);
        let syms: Vec<_> = parsed
            .ast
            .items
            .iter()
            .filter_map(|it| top_level_document_symbol(it, &rope))
            .collect();

        // Widget (class) with a field + method child; Color (enum) with 2
        // variants; main (function).
        let widget = syms.iter().find(|s| s.name == "Widget").expect("Widget");
        assert_eq!(widget.kind, SymbolKind::CLASS);
        let kids = widget.children.as_ref().expect("members");
        assert!(kids.iter().any(|c| c.name == "w" && c.kind == SymbolKind::FIELD));
        assert!(kids.iter().any(|c| c.name == "area" && c.kind == SymbolKind::METHOD));

        let color = syms.iter().find(|s| s.name == "Color").expect("Color");
        assert_eq!(color.kind, SymbolKind::ENUM);
        assert_eq!(color.children.as_ref().map(|c| c.len()), Some(2));

        assert!(syms.iter().any(|s| s.name == "main" && s.kind == SymbolKind::FUNCTION));
    }

    /// `current_package` reads the file's own package so auto-import can skip
    /// same-package (and self) types.
    #[test]
    fn current_package_is_read_for_same_package_suppression() {
        assert_eq!(current_package("package xss.it;\npublic class Other {}").as_deref(), Some("xss.it"));
        assert_eq!(current_package("  package a.b.c ;\n").as_deref(), Some("a.b.c"));
        assert_eq!(current_package("// a comment\npackage p;\n").as_deref(), Some("p"));
        assert_eq!(current_package("public class NoPkg {}"), None);
    }

    // ---- auto-import edit construction ----

    #[test]
    fn import_edit_inserts_after_package_line_and_dedupes() {
        let rope = Rope::from_str("package shop;\npublic class A {}\n");
        let edit = import_edit(&rope, "a.b.C").expect("should produce an edit");
        // No blank after the package yet → insert one, then the import, so the
        // result is `package shop;` / <blank> / `import a.b.C;`.
        assert_eq!(edit.new_text, "\nimport a.b.C;\n");
        // Inserted at the start of line 1 (just after the package line).
        assert_eq!(edit.range.start.line, 1);
        assert_eq!(edit.range.start.character, 0);

        // Already-imported → no edit.
        let rope2 = Rope::from_str("package shop;\nimport a.b.C;\nclass A {}\n");
        assert!(import_edit(&rope2, "a.b.C").is_none());
    }

    #[test]
    fn import_edit_keeps_single_blank_after_package() {
        // A blank line already separates the package from the import block:
        // the new import joins the block below the blank — no extra blank.
        let rope = Rope::from_str("package shop;\n\nimport a.b.D;\nclass A {}\n");
        let edit = import_edit(&rope, "a.b.C").expect("should produce an edit");
        assert_eq!(edit.new_text, "import a.b.C;\n");
        // Line 2 is the existing `import a.b.D;` — the new line lands there.
        assert_eq!(edit.range.start.line, 2);

        // Package on the file's last line → still gets the separating blank.
        let rope2 = Rope::from_str("package shop;\n");
        let edit2 = import_edit(&rope2, "a.b.C").expect("should produce an edit");
        assert_eq!(edit2.new_text, "\nimport a.b.C;\n");
        assert_eq!(edit2.range.start.line, 1);
    }

    // ---- mid-edit member completion (dangling `.`) ----

    /// A dangling `f.` (nothing after) fails the normal parse, so the live
    /// analysis never types the receiver — which is why a member completion
    /// would otherwise fall back to the statement snippet bag. The reparse
    /// fallback patches the buffer to `f;` and recovers `f`'s type (here a local
    /// of an in-file class), which is what makes the class members appear.
    #[test]
    fn reparse_recovers_receiver_type_for_dangling_dot() {
        let text = "public class Foo { public void bar() {} }\n\
                    public void main() { var f = new Foo(); f. }\n";
        let dot = text.rfind("f.").expect("dangling member access") + 1; // the `.`
        let uri = Url::parse("file:///t.jux").unwrap();
        let ty = receiver_type_by_reparse(text, dot, dot + 1, None, &uri)
            .expect("receiver type recovered from the patched buffer");
        let name = match ty {
            juxc_tycheck::Ty::User { name, .. } => name,
            other => panic!("expected a user type, got {other:?}"),
        };
        assert!(name.ends_with("Foo"), "expected the receiver to type as Foo, got {name}");
    }

    /// A chained `new Other().` — the receiver expression `new Other()` should
    /// type as `Other` so its members (`.flex`, …) resolve while typing, which
    /// is the `uint x = new Other().flex();` shape.
    #[test]
    fn reparse_recovers_chained_new_receiver_type() {
        let text = "public class Other { public void flex() {} }\n\
                    public void main() { var x = new Other(). }\n";
        let dot = text.rfind("). ").expect("chained dot") + 1; // the `.` after `)`
        let uri = Url::parse("file:///t.jux").unwrap();
        let ty = receiver_type_by_reparse(text, dot, dot + 1, None, &uri)
            .expect("chained receiver type recovered");
        let name = match ty {
            juxc_tycheck::Ty::User { name, .. } => name,
            other => panic!("expected a user type, got {other:?}"),
        };
        assert!(name.ends_with("Other"), "expected receiver to type as Other, got {name}");
    }

    // ---- `new <Type>` position + call-site detection ----

    #[test]
    fn is_new_context_detects_constructor_position() {
        let t = "var x = new Foo";
        assert!(is_new_context(t, t.rfind("Foo").unwrap()));
        // A plain reference, not after `new`.
        let t2 = "var x = Foo";
        assert!(!is_new_context(t2, t2.rfind("Foo").unwrap()));
        // Member access, not `new`.
        let t3 = "obj.bar";
        assert!(!is_new_context(t3, t3.rfind("bar").unwrap()));
        // `newThing` is one identifier — not the `new` keyword.
        let t4 = "var x = newThing";
        assert!(!is_new_context(t4, t4.rfind("newThing").unwrap()));
    }

    #[test]
    fn find_enclosing_call_tracks_active_parameter() {
        // Caret after `b` → second argument of `foo(`.
        let t = "foo(a, b";
        assert_eq!(find_enclosing_call(t, t.len()), Some((3, 1)));
        // Caret in an empty argument list → first parameter.
        let t2 = "bar()";
        assert_eq!(find_enclosing_call(t2, 4), Some((3, 0)));
        // Not inside any call.
        let t3 = "x = 1;";
        assert_eq!(find_enclosing_call(t3, t3.len()), None);
        // Nested: caret inside the inner call's args (commas of the outer call
        // don't count).
        let t4 = "foo(a, bar(b";
        assert_eq!(find_enclosing_call(t4, t4.len()), Some((10, 0)));
    }

    #[test]
    fn import_edit_inserts_at_top_without_package() {
        let rope = Rope::from_str("public class A {}\n");
        let edit = import_edit(&rope, "a.b.C").unwrap();
        assert_eq!(edit.range.start.line, 0);
        assert_eq!(edit.new_text, "import a.b.C;\n");
    }

    // ---- FEATURE 2: member completion uses the receiver's inferred type ----

    /// End-to-end: a local typed `SomeClass` produces a `Ty::User` whose span
    /// ends at the `.`, and `members_of` returns that class's members only.
    #[test]
    fn member_completion_resolves_receiver_type() {
        let root = temp_root("member_completion");
        let src = "package shop;\n\
            public class Greeter {\n\
                public String greet(String who) { return who; }\n\
                int count;\n\
            }\n\
            public void run() { var g = new Greeter(); g.greet(\"hi\"); }\n";
        let file = root.join("Greeter.jux");
        fs::write(&file, src).unwrap();
        let uri = Url::from_file_path(&file).unwrap();
        let rope = Rope::from_str(src);
        let analysis = analyze_workspace(&root, &uri, &rope);

        let doc = Document {
            rope: rope.clone(),
            version: 1,
            expr_types: analysis.expr_types,
            type_names: analysis.type_names,
            symbols: analysis.symbols,
            source_paths: analysis.source_paths,
        };

        // Find the `.` of `g.greet` in the source and resolve the receiver `g`.
        let dot = src.rfind("g.greet").unwrap() + 1; // offset of `.`
        let ty = doc
            .type_ending_at(dot)
            .expect("receiver `g` must have an inferred type");
        let members = crate::intel::members_of(&doc.symbols, ty);
        let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"greet"), "expected greet, got {names:?}");
        assert!(names.contains(&"count"), "expected count, got {names:?}");
        assert!(!names.contains(&"run"), "unrelated `run` leaked: {names:?}");

        let _ = fs::remove_dir_all(&root);
    }

    // ---- FEATURE 3: auto-import maps a known type to its package ----

    /// A workspace-known type carries its declaring package, and the import edit
    /// for it inserts `import a.b.C;`.
    #[test]
    fn workspace_index_carries_type_package_for_auto_import() {
        let root = temp_root("auto_import");
        // The type to import lives in package `a.b`.
        fs::write(
            root.join("Widget.jux"),
            "package a.b; public class Widget { public void use() {} }",
        )
        .unwrap();
        // A consumer file that references `Widget` WITHOUT importing it.
        let main = root.join("main.jux");
        fs::write(&main, "package app; public void run() { var w = new Widget(); }").unwrap();

        let index = index_workspace(&root, &Default::default());
        let pkgs = index
            .type_packages
            .get("Widget")
            .expect("Widget must carry a declaring package");
        assert_eq!(pkgs, &vec!["a.b".to_string()]);

        // The import edit for the consumer file inserts the right line, with a
        // blank line separating it from the package declaration.
        let rope = Rope::from_str("package app;\npublic void run() {}\n");
        let edit = import_edit(&rope, "a.b.Widget").expect("should produce an edit");
        assert_eq!(edit.new_text, "\nimport a.b.Widget;\n");

        let _ = fs::remove_dir_all(&root);
    }
}
