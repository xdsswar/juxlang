//! The analysis pass: run the Jux front end over one document and shape the
//! result into what the LSP serves.
//!
//! This is the only place that calls into the compiler. It uses the
//! backend-free [`juxc_driver::check`] entry, which lexes, parses, resolves,
//! and type-checks (auto-prepending the stdlib) but never lowers to Rust — so
//! re-analysing on every keystroke costs nothing in codegen or `cargo`.

use juxc_source::{SourceFile, Span};
use juxc_tycheck::{SymbolTable, Ty};
use ropey::Rope;
use tower_lsp::lsp_types::{Diagnostic, Url};

use crate::diagnostics::to_lsp;

/// Everything one analysis pass produces for a document.
pub struct Analysis {
    /// Diagnostics already mapped to LSP form, ready to publish.
    pub diagnostics: Vec<Diagnostic>,
    /// Per-expression types for hover.
    pub expr_types: Vec<(Span, Ty)>,
    /// In-scope type names for completion.
    pub type_names: Vec<String>,
}

/// Analyse the document at `uri` with current text `rope`.
pub fn analyze(uri: &Url, rope: &Rope) -> Analysis {
    let text = rope.to_string();

    // Prefer a real filesystem path for the SourceFile (nicer in any
    // rendered diagnostic); fall back to the raw URI for untitled buffers.
    let path = uri
        .to_file_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| uri.to_string());

    let result = juxc_driver::check(SourceFile::new(path, text));

    // The stdlib is error-free by construction, so every diagnostic here
    // belongs to the open document; map them all (§L.7).
    let diagnostics = result
        .diagnostics
        .iter()
        .map(|d| to_lsp(rope, uri, d))
        .collect();

    let expr_types: Vec<(Span, Ty)> = result.expr_types.into_iter().collect();

    let mut type_names = Vec::new();
    collect_type_names(&result.symbols, &mut type_names);

    Analysis { diagnostics, expr_types, type_names }
}

/// Collect the bare (last-segment) names of every type and free function the
/// symbol table knows about, deduplicated. These feed completion so the user
/// can write `Map`, `List`, `String`, their own classes, etc. by short name.
fn collect_type_names(symbols: &SymbolTable, out: &mut Vec<String>) {
    let mut push_last = |fqn: &str| {
        let bare = fqn.rsplit('.').next().unwrap_or(fqn);
        let bare = bare.to_string();
        if !out.contains(&bare) {
            out.push(bare);
        }
    };
    for k in symbols.classes.keys() {
        push_last(k);
    }
    for k in symbols.records.keys() {
        push_last(k);
    }
    for k in symbols.enums.keys() {
        push_last(k);
    }
    for k in symbols.interfaces.keys() {
        push_last(k);
    }
    for k in symbols.functions.keys() {
        push_last(k);
    }
    out.sort();
}
