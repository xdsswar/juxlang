//! End-to-end tests for references, rename, semantic tokens, inlay hints,
//! workspace symbols, source roots and the document store: each analyses a
//! real temp project and asks the feature's function what the editor would.

use std::fs;
use std::path::{Path, PathBuf};

use ropey::Rope;
use tower_lsp::lsp_types::*;

use crate::analysis::{analyze_workspace, analyze_workspace_in};
use crate::doc::Document;
use crate::position::PositionEncoding;
use crate::roots::{RootKind, RootOrigin, SourceRoot, SourceRoots};

const U16: PositionEncoding = PositionEncoding::Utf16;

/// A temp project directory with a `jux.toml`, so its files form one
/// compilation set.
fn project(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("juxc_lsp_e2e_{}_{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("jux.toml"), "[package]\nname = \"t\"\nversion = \"0.1.0\"\n").unwrap();
    dir
}

/// Write `files` under `root`, then open `open` (one of them) the way the
/// server does: analysed together with the rest.
fn open(root: &Path, files: &[(&str, &str)], open: &str) -> (Document, Url) {
    for (name, text) in files {
        let path = root.join(name);
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).unwrap();
        }
        fs::write(path, text).unwrap();
    }
    let text = files.iter().find(|(n, _)| *n == open).expect("the opened file").1;
    let uri = Url::from_file_path(root.join(open)).unwrap();
    let rope = Rope::from_str(text);
    let analysis = analyze_workspace(root, &uri, &rope);
    (Document::analysed(rope, 1, U16, analysis), uri)
}

/// The position of the `nth` occurrence of `needle` in `text` (plus `delta`
/// bytes), in UTF-16 columns.
fn pos(text: &str, needle: &str, nth: usize, delta: usize) -> Position {
    let offset = text.match_indices(needle).nth(nth).expect("needle present").0 + delta;
    crate::position::offset_to_position(&Rope::from_str(text), offset, U16)
}

/// The text each location covers, as `file:text` for readable assertions.
fn covered(locations: &[Location], root: &Path) -> Vec<String> {
    let mut out: Vec<String> = locations
        .iter()
        .map(|l| {
            let path = l.uri.to_file_path().unwrap();
            let text = fs::read_to_string(&path).unwrap();
            let rope = Rope::from_str(&text);
            let s = crate::position::position_to_offset(&rope, l.range.start, U16);
            let e = crate::position::position_to_offset(&rope, l.range.end, U16);
            let name = path.strip_prefix(root).unwrap().display().to_string().replace('\\', "/");
            format!("{name}@{s}:{}", &text[s..e])
        })
        .collect();
    out.sort();
    out
}

// ---- references ----------------------------------------------------------

/// A local's references are its own uses, and a same-named local in another
/// function is a different variable.
#[test]
fn references_to_a_local_stay_in_its_scope() {
    let root = project("refs_local");
    let src = "public void a() {\n    var total = 1;\n    print(total + total);\n}\npublic void b() {\n    var total = 2;\n    print(total);\n}\n";
    let (doc, uri) = open(&root, &[("main.jux", src)], "main.jux");
    let refs = crate::references::references(&doc, &uri, pos(src, "total", 1, 0), true);
    assert_eq!(refs.len(), 3, "{:?}", covered(&refs, &root));
    let without_decl = crate::references::references(&doc, &uri, pos(src, "total", 1, 0), false);
    assert_eq!(without_decl.len(), 2, "the declaration drops out: {:?}", covered(&without_decl, &root));
    let _ = fs::remove_dir_all(&root);
}

/// A method's references span files: the declaration, a call through a typed
/// receiver in another file, and an implicit-`this` call in its own class. A
/// same-named method of an unrelated class is not one of them.
#[test]
fn references_to_a_member_span_the_project() {
    let root = project("refs_member");
    let shop = "package shop;\npublic class Cart {\n    public int total() { return 1; }\n    public int twice() { return total() + total(); }\n}\npublic class Other {\n    public int total() { return 2; }\n}\n";
    let main = "import shop.Cart;\nimport shop.Other;\npublic void run() {\n    var c = new Cart();\n    var o = new Other();\n    print(c.total());\n    print(o.total());\n}\n";
    let (doc, uri) = open(&root, &[("shop/Cart.jux", shop), ("main.jux", main)], "main.jux");
    let refs = crate::references::references(&doc, &uri, pos(main, "c.total", 0, 2), true);
    let got = covered(&refs, &root);
    assert_eq!(got.len(), 4, "declaration, two implicit calls, one qualified call: {got:?}");
    assert!(got.iter().any(|g| g.starts_with("main.jux")), "{got:?}");
    assert!(
        !got.iter().any(|g| g.contains(&format!("@{}", main.find("o.total").unwrap() + 2))),
        "`Other.total` is a different method: {got:?}"
    );
    let _ = fs::remove_dir_all(&root);
}

/// A type's references include its import, a `new`, a type position and the
/// declaration, across files.
#[test]
fn references_to_a_type_include_imports_and_uses() {
    let root = project("refs_type");
    let lib = "package lib;\npublic class Widget {\n    public Widget() {}\n}\n";
    let main = "import lib.Widget;\npublic void run() {\n    Widget w = new Widget();\n}\n";
    let (doc, uri) = open(&root, &[("lib/Widget.jux", lib), ("main.jux", main)], "main.jux");
    let refs = crate::references::references(&doc, &uri, pos(main, "Widget w", 0, 0), true);
    let got = covered(&refs, &root);
    assert!(got.iter().filter(|g| g.starts_with("main.jux")).count() >= 3, "import + type + new: {got:?}");
    assert!(got.iter().any(|g| g.starts_with("lib/Widget.jux")), "the declaration: {got:?}");
    let _ = fs::remove_dir_all(&root);
}

// ---- rename ----------------------------------------------------------------

/// The text of `edit` applied to `uri`'s file (plain `changes` form).
fn apply(edit: &WorkspaceEdit, uri: &Url, original: &str) -> String {
    let edits = edit.changes.as_ref().and_then(|c| c.get(uri)).cloned().unwrap_or_default();
    let mut rope = Rope::from_str(original);
    let mut sorted = edits;
    sorted.sort_by_key(|e| std::cmp::Reverse((e.range.start.line, e.range.start.character)));
    for e in sorted {
        let change = TextDocumentContentChangeEvent { range: Some(e.range), range_length: None, text: e.new_text };
        crate::sync::apply_changes(&mut rope, &[change], U16);
    }
    rope.to_string()
}

#[test]
fn renaming_a_local_rewrites_its_uses_only() {
    let root = project("rename_local");
    let src = "public void a() {\n    var n = 1;\n    print(n + n);\n}\npublic void b() {\n    var n = 2;\n}\n";
    let (doc, uri) = open(&root, &[("main.jux", src)], "main.jux");
    let edit = crate::references::rename(&doc, &uri, pos(src, "n + n", 0, 0), "count", false)
        .unwrap()
        .unwrap();
    let out = apply(&edit, &uri, src);
    assert_eq!(out, "public void a() {\n    var count = 1;\n    print(count + count);\n}\npublic void b() {\n    var n = 2;\n}\n");
    let _ = fs::remove_dir_all(&root);
}

/// Rename refuses what would break the program: a keyword, a non-identifier,
/// a name already in scope, and a declaration outside the project.
#[test]
fn rename_refuses_bad_names_conflicts_and_library_symbols() {
    let root = project("rename_refuse");
    let src = "public void run() {\n    var a = 1;\n    var b = 2;\n    print(a + b);\n    var v = new Vec<int>();\n}\n";
    let (doc, uri) = open(&root, &[("main.jux", src)], "main.jux");
    let at_a = pos(src, "a = 1", 0, 0);
    assert!(crate::references::rename(&doc, &uri, at_a, "class", false).unwrap_err().contains("keyword"));
    assert!(crate::references::rename(&doc, &uri, at_a, "9lives", false).unwrap_err().contains("identifier"));
    assert!(crate::references::rename(&doc, &uri, at_a, "int", false).unwrap_err().contains("built-in"));
    let clash = crate::references::rename(&doc, &uri, at_a, "b", false).unwrap_err();
    assert!(clash.contains("already"), "{clash}");
    // `Vec` is the standard library's.
    let lib = crate::references::prepare_rename(&doc, pos(src, "Vec", 0, 0));
    assert!(lib.is_err(), "the standard library is not renamable: {lib:?}");
    let _ = fs::remove_dir_all(&root);
}

/// Prepare-rename answers with the name's range and text.
#[test]
fn prepare_rename_offers_the_name() {
    let root = project("prepare");
    let src = "public void run() {\n    var total = 1;\n    print(total);\n}\n";
    let (doc, _) = open(&root, &[("main.jux", src)], "main.jux");
    match crate::references::prepare_rename(&doc, pos(src, "total", 1, 2)).unwrap() {
        Some(PrepareRenameResponse::RangeWithPlaceholder { placeholder, range }) => {
            assert_eq!(placeholder, "total");
            assert_eq!(range.start, pos(src, "total", 1, 0));
        }
        other => panic!("a range with the name: {other:?}"),
    }
    let _ = fs::remove_dir_all(&root);
}

/// Renaming a public type declared in the file named after it moves the file
/// too (§3.1), when the client can apply file renames; otherwise plain text
/// edits, in every file that mentions it.
#[test]
fn renaming_a_type_renames_its_file_when_the_client_can() {
    let root = project("rename_type");
    let lib = "package lib;\npublic class Widget {\n}\n";
    let main = "import lib.Widget;\npublic void run() {\n    var w = new Widget();\n}\n";
    let (doc, uri) = open(&root, &[("lib/Widget.jux", lib), ("main.jux", main)], "main.jux");
    let at = pos(main, "new Widget", 0, 4);

    let plain = crate::references::rename(&doc, &uri, at, "Gadget", false).unwrap().unwrap();
    assert_eq!(apply(&plain, &uri, main), "import lib.Gadget;\npublic void run() {\n    var w = new Gadget();\n}\n");
    let lib_uri = Url::from_file_path(root.join("lib/Widget.jux")).unwrap();
    assert_eq!(apply(&plain, &lib_uri, lib), "package lib;\npublic class Gadget {\n}\n");

    let with_file = crate::references::rename(&doc, &uri, at, "Gadget", true).unwrap().unwrap();
    let Some(DocumentChanges::Operations(ops)) = with_file.document_changes else {
        panic!("document changes with a file rename expected");
    };
    let renamed = ops.iter().any(|op| match op {
        DocumentChangeOperation::Op(ResourceOp::Rename(r)) => {
            r.old_uri == lib_uri && r.new_uri.path().ends_with("/lib/Gadget.jux")
        }
        _ => false,
    });
    assert!(renamed, "the file follows the type: {ops:?}");
    let _ = fs::remove_dir_all(&root);
}

/// A member rename that would collide with an existing member is refused.
#[test]
fn a_member_rename_onto_an_existing_member_is_refused() {
    let root = project("rename_member_clash");
    let src = "public class Cart {\n    public int total() { return 1; }\n    public int count() { return 2; }\n}\n";
    let (doc, uri) = open(&root, &[("Cart.jux", src)], "Cart.jux");
    let err = crate::references::rename(&doc, &uri, pos(src, "total", 0, 0), "count", false).unwrap_err();
    assert!(err.contains("already has a member"), "{err}");
    let _ = fs::remove_dir_all(&root);
}

// ---- semantic tokens ---------------------------------------------------------

/// Identifiers are classified by what they are: a class, a parameter, a
/// local, a method, a field, an enum member, a namespace, a type parameter.
#[test]
fn semantic_tokens_classify_identifiers() {
    let root = project("semtok");
    let src = "package app;\npublic enum Color { Red, Green }\npublic class Box<T> {\n    public int size;\n    public int grow(int by) {\n        var next = size + by;\n        return next;\n    }\n}\npublic void run() {\n    var b = new Box<int>();\n    print(b.grow(1));\n    var c = Color.Red;\n}\n";
    let (doc, _) = open(&root, &[("app/Main.jux", src)], "app/Main.jux");
    let tokens = crate::semtok::classify(&doc);
    let kind_at = |needle: &str, nth: usize| -> Option<SemanticTokenType> {
        let start = src.match_indices(needle).nth(nth)?.0;
        let t = tokens.iter().find(|t| t.start == start)?;
        Some(crate::semtok::TOKEN_TYPES[t.token_type as usize].clone())
    };
    assert_eq!(kind_at("app", 0), Some(SemanticTokenType::NAMESPACE));
    assert_eq!(kind_at("Box<int>", 0), Some(SemanticTokenType::CLASS));
    assert_eq!(kind_at("T>", 0), Some(SemanticTokenType::TYPE_PARAMETER));
    assert_eq!(kind_at("by)", 0), Some(SemanticTokenType::PARAMETER));
    assert_eq!(kind_at("next", 1), Some(SemanticTokenType::VARIABLE));
    assert_eq!(kind_at("size +", 0), Some(SemanticTokenType::PROPERTY));
    assert_eq!(kind_at("grow(1)", 0), Some(SemanticTokenType::METHOD));
    assert_eq!(kind_at("Red;", 0), Some(SemanticTokenType::ENUM_MEMBER));
    assert_eq!(kind_at("Color.Red", 0), Some(SemanticTokenType::ENUM));
    // A declaration carries the declaration modifier.
    let decl = tokens.iter().find(|t| t.start == src.find("next =").unwrap()).unwrap();
    assert_eq!(decl.modifiers & 1, 1, "declaration bit");
    let _ = fs::remove_dir_all(&root);
}

/// Delta encoding: positions are relative to the previous token, on the wire
/// in the negotiated unit.
#[test]
fn semantic_tokens_are_delta_encoded() {
    let root = project("semtok_delta");
    let src = "public void run(int a) {\n    print(a);\n}\n";
    let (doc, _) = open(&root, &[("main.jux", src)], "main.jux");
    let data = crate::semtok::semantic_tokens(&doc, None).data;
    let mut line = 0;
    let mut col = 0;
    let mut absolute = Vec::new();
    for t in &data {
        line += t.delta_line;
        col = if t.delta_line == 0 { col + t.delta_start } else { t.delta_start };
        absolute.push((line, col, t.length));
    }
    assert!(absolute.contains(&(0, 20, 1)), "the parameter `a` at 0:20: {absolute:?}");
    assert!(absolute.contains(&(1, 10, 1)), "its use at 1:10: {absolute:?}");
    let _ = fs::remove_dir_all(&root);
}

// ---- inlay hints -------------------------------------------------------------

#[test]
fn inlay_hints_show_var_types_and_parameter_names() {
    let root = project("inlay");
    let src = "public int area(int width, int height) { return width * height; }\npublic void run() {\n    var a = area(3, 4);\n    int width = 2;\n    print(area(width, 5));\n}\n";
    let (doc, _) = open(&root, &[("main.jux", src)], "main.jux");
    let all = Range::new(Position::new(0, 0), Position::new(99, 0));
    let hints = crate::inlay::inlay_hints(&doc, all);
    let labels: Vec<String> = hints
        .iter()
        .map(|h| match &h.label {
            InlayHintLabel::String(s) => s.clone(),
            InlayHintLabel::LabelParts(p) => p.iter().map(|x| x.value.clone()).collect(),
        })
        .collect();
    assert!(labels.contains(&": int".to_string()), "the `var` type: {labels:?}");
    assert_eq!(labels.iter().filter(|l| *l == "width:").count(), 1, "`width` passed as `width` needs no hint: {labels:?}");
    assert_eq!(labels.iter().filter(|l| *l == "height:").count(), 2, "{labels:?}");
    let _ = fs::remove_dir_all(&root);
}

// ---- workspace symbols ---------------------------------------------------------

#[test]
fn workspace_symbols_find_project_declarations_fuzzily() {
    let root = project("wsym");
    let src = "package shop;\npublic class ShoppingCart {\n    public int total() { return 1; }\n}\n";
    let (doc, _) = open(&root, &[("shop/ShoppingCart.jux", src)], "shop/ShoppingCart.jux");
    let src_view = crate::symbols::SymbolSource {
        symbols: &doc.symbols,
        paths: &doc.source_paths,
        texts: &doc.source_texts,
    };
    let found = crate::symbols::workspace_symbols(&src_view, "shcart", U16);
    let cart = found.iter().find(|s| s.name == "ShoppingCart").expect("fuzzy match on the class");
    assert_eq!(cart.kind, SymbolKind::CLASS);
    assert_eq!(cart.container_name.as_deref(), Some("shop"));
    let total = crate::symbols::workspace_symbols(&src_view, "total", U16);
    assert!(total.iter().any(|s| s.name == "total" && s.container_name.as_deref() == Some("ShoppingCart")));
    // The standard library is not the project's.
    assert!(crate::symbols::workspace_symbols(&src_view, "print", U16).iter().all(|s| s.name != "print"));
    let _ = fs::remove_dir_all(&root);
}

// ---- source roots and the document store ---------------------------------------

/// Marked roots with no `jux.toml` form one compilation set, so a type from
/// the other root resolves (goto-definition works) where without the roots
/// each file would be a program on its own.
#[test]
fn marked_roots_give_cross_file_analysis_without_a_manifest() {
    let base = std::env::temp_dir().join(format!("juxc_lsp_e2e_{}_roots", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(base.join("core/model")).unwrap();
    fs::create_dir_all(base.join("app")).unwrap();
    fs::write(base.join("core/model/Item.jux"), "package model;\npublic class Item {}\n").unwrap();
    let main = "import model.Item;\npublic void run() { var i = new Item(); }\n";
    fs::write(base.join("app/main.jux"), main).unwrap();
    let uri = Url::from_file_path(base.join("app/main.jux")).unwrap();
    let rope = Rope::from_str(main);

    let alone = analyze_workspace(&base, &uri, &rope);
    assert!(alone.symbols.definition_of("Item").is_none(), "without roots the file is on its own");

    let roots = SourceRoots::new(vec![
        SourceRoot { path: base.join("core"), kind: RootKind::Sources, origin: RootOrigin::Marked },
        SourceRoot { path: base.join("app"), kind: RootKind::Sources, origin: RootOrigin::Marked },
    ]);
    let together = analyze_workspace_in(&base, &uri, &rope, &roots, &Default::default(), U16);
    let doc = Document::analysed(rope, 1, U16, together);
    let def = crate::definition::goto_definition(&doc, &uri, pos(main, "new Item", 0, 4));
    let Some(GotoDefinitionResponse::Scalar(loc)) = def else { panic!("definition via the roots: {def:?}") };
    assert!(loc.uri.path().ends_with("/core/model/Item.jux"), "{loc:?}");
    assert_eq!(roots.package_of(&base.join("core/model/Item.jux")).as_deref(), Some("model"));
    let _ = fs::remove_dir_all(&base);
}

/// An analysis of an older revision never overwrites a newer one's caches.
#[test]
fn a_stale_analysis_is_not_installed() {
    let root = project("stale");
    let src = "public void run() { var x = 1; }\n";
    let (mut doc, uri) = open(&root, &[("main.jux", src)], "main.jux");
    doc.version = 7; // the buffer moved on
    let older = analyze_workspace(&root, &uri, &Rope::from_str(src));
    assert!(!doc.install(6, older), "revision 6's analysis is stale");
    let current = analyze_workspace(&root, &uri, &Rope::from_str(src));
    assert!(doc.install(7, current));
    assert_eq!(doc.analysed_version, 7);
    let _ = fs::remove_dir_all(&root);
}

/// Hover and member lookups only read the open document's own expression
/// types: another file's expression at the same offsets must not answer.
#[test]
fn expression_types_are_read_for_the_open_file_only() {
    let root = project("own_types");
    // The two reads of `s` and `n` sit at the same byte offsets in their files.
    let a = "public void a() { var s = \"t\"; var q = s; }\n";
    let b = "public void b() { var n = 77;  var q = n; }\n";
    assert_eq!(a.find("= s;"), b.find("= n;"), "the fixture relies on equal offsets");
    let (doc, _) = open(&root, &[("a.jux", a), ("b.jux", b)], "b.jux");
    let offset = b.find("= n;").unwrap() + 2;
    let (_, ty) = doc.type_at(offset).expect("a type at the read of `n`");
    assert_eq!(ty.to_string(), "int", "b.jux's own `n`, not a.jux's `s` at the same offset");
    let _ = fs::remove_dir_all(&root);
}
