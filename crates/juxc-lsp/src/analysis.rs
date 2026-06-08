//! The analysis pass: run the Jux front end over the workspace and shape the
//! result into what the LSP serves.
//!
//! This is the only place that calls into the compiler. It uses the
//! backend-free [`juxc_driver::check_workspace`] entry, which lexes, parses,
//! resolves, and type-checks (auto-prepending the stdlib) but never lowers to
//! Rust — so re-analysing on every keystroke costs nothing in codegen or
//! `cargo`.
//!
//! ## Why workspace-wide, not single-file
//!
//! A single-file check resolves a cross-file imported type *by name* but sees
//! an empty method table for it, producing false `[E0413] no method …`
//! diagnostics (the imported class' body lives in another file). Checking the
//! whole workspace together — the open buffer's live text plus every other
//! `.jux` on disk — gives every unit the merged symbol table the batch
//! compiler uses, so those false errors disappear. Diagnostics come back
//! tagged with a `file` index (see `juxc-diagnostics`) so we can publish each
//! one against the correct document URI.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use juxc_source::{SourceFile, Span};
use juxc_tycheck::{SymbolTable, Ty};
use ropey::Rope;
use tower_lsp::lsp_types::{Diagnostic, Url};

use crate::diagnostics::to_lsp;
use crate::workspace::scan_jux_files;

/// Everything one analysis pass produces.
pub struct Analysis {
    /// Diagnostics grouped by the document they belong to, already mapped to
    /// LSP form and ready to publish. Every workspace file that was analysed
    /// appears as a key (with an empty vec when it has no diagnostics) so the
    /// server can clear stale diagnostics on files that just became clean.
    pub diagnostics_by_uri: HashMap<Url, Vec<Diagnostic>>,
    /// Per-expression types for hover, for the *open* document only.
    pub expr_types: Vec<(Span, Ty)>,
    /// In-scope type names for completion (from the merged symbol table).
    pub type_names: Vec<String>,
}

/// Analyse the open document at `uri` (current text `rope`) **in the context
/// of its workspace** `root`.
///
/// All `.jux` files under `root` are gathered; the open document's live text
/// overrides whatever is on disk for its own path, every other file is read
/// from disk. The whole set is checked together via `check_workspace`, and the
/// returned diagnostics — tagged with a `file` index — are grouped by URI and
/// resolved against the *right* file's text (not always the open rope).
pub fn analyze_workspace(root: &Path, uri: &Url, rope: &Rope) -> Analysis {
    // The open document's filesystem path (used to override its on-disk text).
    let open_path: Option<PathBuf> = uri.to_file_path().ok();

    // Gather every workspace `.jux` file. Use the open buffer's live text for
    // its own path; read the rest from disk. This mirrors
    // `workspace::index_workspace`'s `overrides` handling.
    let mut sources: Vec<SourceFile> = Vec::new();
    let mut saw_open = false;
    for path in scan_jux_files(root) {
        if open_path.as_deref() == Some(path.as_path()) {
            sources.push(SourceFile::new(path, rope.to_string()));
            saw_open = true;
        } else {
            let text = std::fs::read_to_string(&path).unwrap_or_default();
            sources.push(SourceFile::new(path, text));
        }
    }
    // If the open document isn't under the root (or hasn't hit disk yet),
    // analyse it alongside the rest so it still gets diagnostics.
    if !saw_open {
        let path = open_path
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| uri.to_string());
        sources.push(SourceFile::new(path, rope.to_string()));
    }

    analyze_sources(uri, rope, sources)
}

/// Single-file fallback used when there's no workspace root (untitled buffers,
/// loose files opened outside any project). Behaves like the legacy path: the
/// open document plus the auto-loaded stdlib.
pub fn analyze_single(uri: &Url, rope: &Rope) -> Analysis {
    let path = uri
        .to_file_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| uri.to_string());
    let source = SourceFile::new(path, rope.to_string());
    analyze_sources(uri, rope, vec![source])
}

/// Shared core: feed `sources` (the user units; stdlib is auto-prepended) to
/// `check_workspace`, then group the tagged diagnostics by URI and resolve
/// each span against its own file's text.
fn analyze_sources(open_uri: &Url, open_rope: &Rope, sources: Vec<SourceFile>) -> Analysis {
    let result = juxc_driver::check_workspace(sources);

    // Pre-seed every analysed *user* file's URI with an empty vec so a file
    // that went from "had errors" to "clean" gets its diagnostics cleared
    // (publishing an empty list is how LSP clears). We skip stdlib units —
    // they live in-memory under synthetic paths and are never opened.
    let mut by_uri: HashMap<Url, Vec<Diagnostic>> = HashMap::new();
    for src in &result.sources {
        if let Some(url) = source_url(src, open_uri) {
            by_uri.entry(url).or_default();
        }
    }

    // Map each diagnostic to its owning file's URI and resolve its span
    // against that file's text. The open document reuses the live rope (so a
    // span lands on the unsaved edit); other files build a rope from the
    // SourceFile contents we just checked.
    for d in &result.diagnostics {
        let Some(idx) = d.file else { continue };
        let Some(src) = result.sources.get(idx) else { continue };
        let Some(url) = source_url(src, open_uri) else { continue };
        let rope = if &url == open_uri {
            open_rope.clone()
        } else {
            Rope::from_str(src.contents())
        };
        let lsp = to_lsp(&rope, &url, d);
        by_uri.entry(url).or_default().push(lsp);
    }

    let expr_types: Vec<(Span, Ty)> = result.expr_types.into_iter().collect();
    let mut type_names = Vec::new();
    collect_type_names(&result.symbols, &mut type_names);

    Analysis { diagnostics_by_uri: by_uri, expr_types, type_names }
}

/// Resolve a checked `SourceFile` to the `Url` we publish diagnostics under.
///
/// Stdlib units carry synthetic paths that don't exist on disk; `from_file_path`
/// fails for those, so they're filtered out (we never publish stdlib
/// diagnostics — the stdlib is error-free by construction anyway). The open
/// document's own URI is returned verbatim so it matches `open_uri` exactly
/// even if the round-trip through the filesystem path would normalize it.
fn source_url(src: &SourceFile, open_uri: &Url) -> Option<Url> {
    let url = Url::from_file_path(src.path()).ok()?;
    // Normalize to the editor's exact URI when they refer to the same file,
    // so grouping keys match what the editor sent.
    if let Ok(open_path) = open_uri.to_file_path() {
        if open_path == src.path() {
            return Some(open_uri.clone());
        }
    }
    Some(url)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Create a unique temp directory for one test run. Avoids pulling in a
    /// `tempfile` dev-dependency: we just stamp the dir with the process id +
    /// a per-test tag and clean it up at the end.
    fn temp_root(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("juxc_lsp_test_{}_{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp root");
        dir
    }

    const FOLLOW: &str =
        "package xss.it.follow; public class Follow { public void printVal(){ print(1); } }";
    const MAIN: &str = "import xss.it.follow.Follow; \
         public static void main(){ var f = new Follow(); f.printVal(); }";

    /// The bug fix, proven through the LSP analysis layer: analysing `main.jux`
    /// IN ITS WORKSPACE (with `Follow.jux` present) publishes ZERO diagnostics
    /// for the open file — the false `[E0413] no method printVal` is GONE.
    #[test]
    fn workspace_analysis_clears_false_e0413_on_open_file() {
        let root = temp_root("clear_e0413");
        let follow = root.join("Follow.jux");
        let main = root.join("main.jux");
        fs::write(&follow, FOLLOW).unwrap();
        fs::write(&main, MAIN).unwrap();

        let main_uri = Url::from_file_path(&main).unwrap();
        let rope = Rope::from_str(MAIN);
        let analysis = analyze_workspace(&root, &main_uri, &rope);

        let main_diags = analysis
            .diagnostics_by_uri
            .get(&main_uri)
            .expect("main.jux must be in the analysed set");
        assert!(
            main_diags.is_empty(),
            "expected no diagnostics on main.jux, got: {:?}",
            main_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        let _ = fs::remove_dir_all(&root);
    }

    /// An error placed in `Follow.jux` is published against `Follow.jux`'s URI,
    /// NOT against the open `main.jux`.
    #[test]
    fn error_in_other_file_is_reported_against_that_file() {
        let root = temp_root("other_file_error");
        let follow = root.join("Follow.jux");
        let main = root.join("main.jux");
        // `nope()` doesn't exist → a real E0413 inside Follow.jux.
        fs::write(
            &follow,
            "package xss.it.follow; public class Follow { \
             public void printVal(){ print(1); } public void boom(){ this.nope(); } }",
        )
        .unwrap();
        fs::write(&main, MAIN).unwrap();

        let main_uri = Url::from_file_path(&main).unwrap();
        let follow_uri = Url::from_file_path(&follow).unwrap();
        let rope = Rope::from_str(MAIN);
        let analysis = analyze_workspace(&root, &main_uri, &rope);

        // main.jux is clean...
        let main_diags = analysis.diagnostics_by_uri.get(&main_uri).unwrap();
        assert!(
            main_diags.is_empty(),
            "main.jux should be clean, got: {:?}",
            main_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        // ...and Follow.jux carries the error.
        let follow_diags = analysis
            .diagnostics_by_uri
            .get(&follow_uri)
            .expect("Follow.jux must be present in the analysed set");
        assert!(
            follow_diags.iter().any(|d| d.message.contains("nope")),
            "expected the `nope` error on Follow.jux, got: {:?}",
            follow_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        let _ = fs::remove_dir_all(&root);
    }
}
