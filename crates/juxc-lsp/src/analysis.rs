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
    /// The merged workspace symbol table (stdlib + every project unit). Hover
    /// and member-completion resolve the identifier under the cursor against
    /// this — a type name, free function, or a member reached via a receiver's
    /// inferred [`Ty`]. Wrapped in `Arc` so the (potentially large) table can be
    /// cheaply shared into the cached [`crate::doc::Document`].
    pub symbols: std::sync::Arc<SymbolTable>,
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

    Analysis {
        diagnostics_by_uri: by_uri,
        expr_types,
        type_names,
        symbols: std::sync::Arc::new(result.symbols),
    }
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

    // ====================================================================
    // FEATURE 1 — hover signatures over known symbols
    // ====================================================================

    /// A small one-file workspace exercising type / method resolution. The
    /// `Greeter` class has a `greet` method with a typed param + return so the
    /// rendered signature is non-trivial.
    const GREETER: &str = "package shop;\n\
        public class Greeter {\n\
            public String greet(String who) { return who; }\n\
            int count;\n\
        }\n";

    fn analyze_one(tag: &str, name: &str, text: &str) -> (Analysis, Url, Rope) {
        let root = temp_root(tag);
        let file = root.join(name);
        fs::write(&file, text).unwrap();
        let uri = Url::from_file_path(&file).unwrap();
        let rope = Rope::from_str(text);
        let analysis = analyze_workspace(&root, &uri, &rope);
        (analysis, uri, rope)
    }

    /// Hovering a TYPE name resolves to its class-declaration signature.
    #[test]
    fn hover_type_renders_class_signature() {
        let (analysis, _uri, _rope) = analyze_one("hover_type", "Greeter.jux", GREETER);
        let resolved = crate::intel::resolve_type(&analysis.symbols, "Greeter")
            .expect("Greeter must resolve to a type");
        let sig = resolved.signature();
        assert!(
            sig.contains("class Greeter"),
            "expected a class signature, got: {sig}"
        );
        assert!(sig.contains("public"), "expected visibility, got: {sig}");
    }

    /// Hovering a METHOD name (resolved by name in the symbol table) renders its
    /// full signature: return type, name, and typed params.
    #[test]
    fn hover_method_renders_signature_with_params() {
        let (analysis, _uri, _rope) = analyze_one("hover_method", "Greeter.jux", GREETER);
        // Resolve `greet` as a member of a `Greeter`-typed receiver.
        let recv = Ty::User { name: "shop.Greeter".to_string(), generic_args: vec![] };
        let resolved = crate::intel::resolve_member(&analysis.symbols, &recv, "greet")
            .expect("greet must resolve on Greeter");
        let sig = resolved.signature();
        assert!(sig.contains("greet"), "missing method name: {sig}");
        assert!(sig.contains("String"), "missing return/param type: {sig}");
        assert!(sig.contains("(String who)"), "missing typed params: {sig}");
    }

    // ====================================================================
    // FEATURE 2 — receiver members come from the receiver's type only
    // ====================================================================

    /// `members_of` on a `Greeter` receiver lists exactly Greeter's own
    /// method/field names — and nothing unrelated.
    #[test]
    fn members_of_lists_receiver_members_only() {
        let (analysis, _uri, _rope) = analyze_one("members", "Greeter.jux", GREETER);
        let recv = Ty::User { name: "shop.Greeter".to_string(), generic_args: vec![] };
        let members = crate::intel::members_of(&analysis.symbols, &recv);
        let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"greet"), "expected greet, got {names:?}");
        assert!(names.contains(&"count"), "expected count, got {names:?}");
        // No unrelated global leaks in (e.g. stdlib `print` / `main`).
        assert!(
            !names.contains(&"print") && !names.contains(&"main"),
            "unrelated globals leaked into members: {names:?}"
        );
        // The method member is rendered with parameters for the `()` insert.
        let greet = members.iter().find(|m| m.name == "greet").unwrap();
        assert!(greet.is_method, "greet should be a method");
        assert!(greet.detail.contains("(String who)"), "detail: {}", greet.detail);
    }

    // ====================================================================
    // PHASE 5 — Rust-derived symbols (`.jux.d` stubs) surface in completion
    // in Jux syntax (JUX-BINDGEN-ADDENDUM §G.10)
    // ====================================================================

    /// A `.jux.d` declaration stub: a signature-only `Widget` whose
    /// methods/ctor end in `;` (no bodies) — exactly what `juxc bindgen`
    /// emits for a foreign Rust type, viewed in Jux syntax.
    const WIDGET_STUB: &str = "package rust.demo;\n\
        public class Widget {\n\
            public Widget(int w, int h);\n\
            public int area();\n\
            public int width();\n\
        }\n";

    /// User code that imports + constructs the stubbed `Widget`.
    const WIDGET_MAIN: &str = "import rust.demo.Widget;\n\
        public void main() {\n\
            var w = new Widget(2, 3);\n\
            print(w.area());\n\
        }\n";

    /// Member completion after `widget.` lists the STUB's methods — the
    /// Rust-derived API surfaced in Jux syntax. Proof that anything in the
    /// symbol table (including `external` `.jux.d` units) is served by the
    /// same `members_of` the LSP uses for member completion.
    #[test]
    fn stub_member_completion_lists_rust_methods_in_jux_syntax() {
        let root = temp_root("stub_member_completion");
        // The `.jux.d` extension is what marks the unit external; the LSP
        // workspace scan must pick it up (workspace::scan_jux_files).
        let stub_dir = root.join(".jux-stubs").join("rust");
        fs::create_dir_all(&stub_dir).unwrap();
        fs::write(stub_dir.join("demo.jux.d"), WIDGET_STUB).unwrap();
        let main = root.join("main.jux");
        fs::write(&main, WIDGET_MAIN).unwrap();

        let main_uri = Url::from_file_path(&main).unwrap();
        let rope = Rope::from_str(WIDGET_MAIN);
        let analysis = analyze_workspace(&root, &main_uri, &rope);

        // The stub resolved cleanly: no errors on the open file.
        let main_diags = analysis.diagnostics_by_uri.get(&main_uri).cloned().unwrap_or_default();
        assert!(
            main_diags.iter().all(|d| d.severity != Some(tower_lsp::lsp_types::DiagnosticSeverity::ERROR)),
            "stub usage should be error-free, got: {:?}",
            main_diags.iter().map(|d| &d.message).collect::<Vec<_>>()
        );

        // Member completion after `widget.` → the Rust-derived methods.
        let recv = Ty::User { name: "rust.demo.Widget".to_string(), generic_args: vec![] };
        let members = crate::intel::members_of(&analysis.symbols, &recv);
        let names: Vec<&str> = members.iter().map(|m| m.name.as_str()).collect();
        assert!(names.contains(&"area"), "expected `area` from the stub, got {names:?}");
        assert!(names.contains(&"width"), "expected `width` from the stub, got {names:?}");
        // Rendered in Jux syntax: the method detail carries the Jux return type.
        let area = members.iter().find(|m| m.name == "area").unwrap();
        assert!(area.is_method, "area should be a method");

        let _ = fs::remove_dir_all(&root);
    }

    /// Completing the type name `Widget` offers the `rust.demo` import — the
    /// auto-import candidate for the Rust-derived type. Proof that `.jux.d`
    /// stub types feed the workspace index's `type_packages` map.
    #[test]
    fn stub_type_offers_rust_demo_import() {
        use std::collections::HashMap;
        let root = temp_root("stub_auto_import");
        let stub_dir = root.join(".jux-stubs").join("rust");
        fs::create_dir_all(&stub_dir).unwrap();
        fs::write(stub_dir.join("demo.jux.d"), WIDGET_STUB).unwrap();
        // A user file that doesn't import Widget yet — so an import is offered.
        fs::write(root.join("other.jux"), "public void other() {}").unwrap();

        let index = crate::workspace::index_workspace(&root, &HashMap::new());

        assert!(
            index.type_names.contains(&"Widget".to_string()),
            "stub type `Widget` should be in the workspace type index, got {:?}",
            index.type_names
        );
        assert!(
            index.member_names.contains(&"area".to_string()),
            "stub method `area` should be in the member index, got {:?}",
            index.member_names
        );
        let pkgs = index
            .type_packages
            .get("Widget")
            .expect("Widget must have an auto-import package candidate");
        assert!(
            pkgs.contains(&"rust.demo".to_string()),
            "completing `Widget` should offer `import rust.demo.Widget`, got {pkgs:?}"
        );

        let _ = fs::remove_dir_all(&root);
    }
}
