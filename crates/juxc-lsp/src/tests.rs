//! The completion, hover, import and outline tests that used to live at the
//! bottom of `server.rs`, kept together when the handlers moved into their
//! own modules. Each drives the module's plain function directly.

use ropey::Rope;
use tower_lsp::lsp_types::*;

use crate::calls::find_enclosing_call;
use crate::completion::*;
use crate::doc::Document;
use crate::imports::{auto_import_for, current_package, import_edit};
use crate::intel;
use crate::intel::member_decl_span;
use crate::position::PositionEncoding;
use crate::symbols::top_level_document_symbol;
use crate::text::{doc_comment_before, is_new_context, receiver_dot_before, word_at};
use crate::workspace::Workspace;

const U16: PositionEncoding = PositionEncoding::Utf16;

/// A document whose caches come from `doc` but whose text is `src`: what the
/// buffer looks like one edit after its last analysis.
fn with_text(doc: &Document, src: &str) -> Document {
    let mut d = Document::new(Rope::from_str(src), 2, U16);
    d.type_names = doc.type_names.clone();
    d.symbols = doc.symbols.clone();
    d.source_paths = doc.source_paths.clone();
    d
}
use crate::analysis::analyze_workspace;
use crate::workspace::index_workspace;
use std::fs;
use std::path::{Path, PathBuf};

fn temp_root(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!("juxc_lsp_srv_test_{}_{}", std::process::id(), tag));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("create temp root");
    // A multi-file Jux PROJECT is delimited by its manifest, the same
    // way `jux build` decides it — and these fixtures are projects:
    // they check cross-file resolution. Without one the analyser
    // (correctly) treats each file as a standalone program, which is
    // what a folder of unrelated examples is.
    fs::write(dir.join("jux.toml"), "[package]
name = \"test\"
version = \"0.1.0\"
")
        .expect("write test manifest");
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
        .filter_map(|it| top_level_document_symbol(it, &rope, U16))
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
/// A file that opens with a block or doc comment still has its package
/// read. Missing it made every same-package type look like it needed an
/// import — the file's own class included.
#[test]
fn package_is_found_past_a_leading_block_comment() {
    assert_eq!(
        current_package("/** Doc. */
package app.model;
public class A {}").as_deref(),
        Some("app.model"),
    );
    assert_eq!(
        current_package("/*
 * licence
 */
package app.model;
").as_deref(),
        Some("app.model"),
    );
    // A block comment CLOSING onto real code still ends the scan.
    assert_eq!(current_package("/* x */ public class A {}
package nope;"), None);
}

/// A type in the CURRENT package needs no import, so none is offered —
/// which is also what stops a class being offered an import of itself.
#[test]
fn same_package_type_is_offered_no_import() {
    let mut ws = Workspace::default();
    ws.type_packages.insert("Animal".to_string(), vec!["app.model".to_string()]);
    let rope = Rope::from_str("package app.model;
public class Shelter {}
");

    assert!(
        auto_import_for("Animal", &ws, Some("app.model"), &rope).is_none(),
        "a sibling in the same package must not be offered an import",
    );
    assert!(
        auto_import_for("Shelter", &ws, Some("app.model"), &rope).is_none(),
        "a class must never be offered an import of itself",
    );
    // A type in ANOTHER package still is.
    ws.type_packages.insert("Widget".to_string(), vec!["app.ui".to_string()]);
    assert!(auto_import_for("Widget", &ws, Some("app.model"), &rope).is_some());
}
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

    let doc = Document::analysed(rope.clone(), 1, U16, analysis);

    // Find the `.` of `g.greet` in the source and resolve the receiver `g`.
    let dot = src.rfind("g.greet").unwrap() + 1; // offset of `.`
    let ty = doc
        .type_ending_at(dot)
        .expect("receiver `g` must have an inferred type");
    // Complete from the same package — `int count;` is package-private.
    let access = intel::AccessCtx {
        package: Some("shop".to_string()),
        enclosing_class_fqn: None,
    };
    let members = crate::intel::members_of(
        &doc.symbols,
        ty,
        intel::ReceiverKind::Instance,
        &access,
    );
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

// ====================================================================
// Smart completion — the build_completions pipeline end-to-end
// ====================================================================

/// Analyse `src` as `name` inside `root` and build the cached Document the
/// completion handler reads — the same construction `refresh` performs.
fn doc_for(root: &Path, name: &str, src: &str) -> (Document, Url) {
    let file = root.join(name);
    fs::write(&file, src).unwrap();
    let uri = Url::from_file_path(&file).unwrap();
    let rope = Rope::from_str(src);
    let analysis = analyze_workspace(root, &uri, &rope);
    let doc = Document::analysed(rope, 1, U16, analysis);
    (doc, uri)
}

/// Locals and parameters are offered in statement context, ranked above
/// keywords, with the matching one preselected.
#[test]
fn completion_offers_locals_and_params_ranked_first() {
    let root = temp_root("locals_completion");
    let src = "public class Greeter {\n\
                   public String greet(String who) {\n\
                       var greeting = \"hi\";\n\
                       gr\n\
                   }\n\
               }\n";
    let (doc, uri) = doc_for(&root, "Greeter.jux", src);
    let offset = src.rfind("gr\n").unwrap() + 2;
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);

    let greeting = items.iter().find(|i| i.label == "greeting").expect("local offered");
    assert_eq!(greeting.kind, Some(CompletionItemKind::VARIABLE));
    assert!(greeting.sort_text.as_deref().unwrap().starts_with("0_"));
    // `greeting` matches the typed `gr` prefix → preselected.
    assert_eq!(greeting.preselect, Some(true));

    let who = items.iter().find(|i| i.label == "who").expect("param offered");
    assert_eq!(who.detail.as_deref(), Some("String"));

    // Keywords rank below locals. (`while` is also a snippet label, so
    // match on the KEYWORD kind.)
    let kw = items
        .iter()
        .find(|i| i.label == "while" && i.kind == Some(CompletionItemKind::KEYWORD))
        .expect("keyword present");
    assert!(kw.sort_text.as_deref().unwrap().starts_with("4_"));
    assert!(greeting.sort_text < kw.sort_text, "locals sort above keywords");

    // The enclosing class's own method is offered (implicit `this`).
    let greet = items.iter().find(|i| i.label == "greet").expect("own method offered");
    assert!(greet.sort_text.as_deref().unwrap().starts_with("1_"));

    let _ = fs::remove_dir_all(&root);
}

/// `obj.` offers instance members only; `Type.` offers statics only.
#[test]
fn member_completion_splits_static_and_instance() {
    let root = temp_root("static_instance");
    let src = "public class Counter {\n\
                   public static int total() { return 1; }\n\
                   public int n;\n\
                   public int bump() { return this.n; }\n\
               }\n\
               public void main() { var c = new Counter(); c. }\n";
    let (doc, uri) = doc_for(&root, "Counter.jux", src);

    // Instance receiver: `c.` → bump + n, NOT total.
    let offset = src.rfind("c. ").unwrap() + 2;
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"bump"), "instance method offered: {labels:?}");
    assert!(labels.contains(&"n"), "instance field offered: {labels:?}");
    assert!(!labels.contains(&"total"), "static leaked into `obj.`: {labels:?}");

    // Static receiver: `Counter.` → total only.
    let src2 = "public class Counter {\n\
                    public static int total() { return 1; }\n\
                    public int n;\n\
                }\n\
                public void main() { Counter. }\n";
    let (doc2, uri2) = doc_for(&root, "Counter2.jux", src2);
    let offset2 = src2.rfind("Counter. ").unwrap() + "Counter.".len();
    let items2 = build_completions(&doc2, &Workspace::default(), &uri2, offset2);
    let labels2: Vec<&str> = items2.iter().map(|i| i.label.as_str()).collect();
    assert!(labels2.contains(&"total"), "static offered on `Type.`: {labels2:?}");
    assert!(!labels2.contains(&"n"), "instance field leaked into `Type.`: {labels2:?}");

    let _ = fs::remove_dir_all(&root);
}

/// `Color.` lists the enum's variants (the receiver is the type name, no
/// expression type needed — and no reparse either).
#[test]
fn enum_dot_lists_variants() {
    let root = temp_root("enum_variants");
    let src = "public enum Color { Red, Green, Blue }\n\
               public void main() { Color. }\n";
    let (doc, uri) = doc_for(&root, "Color.jux", src);
    let offset = src.rfind("Color. ").unwrap() + "Color.".len();
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    for v in ["Red", "Green", "Blue"] {
        assert!(labels.contains(&v), "variant `{v}` offered: {labels:?}");
    }
    let red = items.iter().find(|i| i.label == "Red").unwrap();
    assert_eq!(red.kind, Some(CompletionItemKind::ENUM_MEMBER));

    let _ = fs::remove_dir_all(&root);
}

/// Methods insert a parameter snippet — the caret lands on the first
/// argument, IntelliJ-style.
 /// Jux takes named arguments, and nothing offered the names -- so using the
/// form meant reading the signature somewhere else first, which is most of
/// the reason to have named arguments at all.
#[test]
fn call_offers_parameter_names() {
    let root = temp_root("named_args");
    let src = "public void greet(String who, int times) { }
               public void run() { greet( }
";
    let (doc, uri) = doc_for(&root, "Named.jux", src);
    let offset = src.find("greet( ").unwrap() + "greet(".len();
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"who:"), "parameter labels offered: {labels:?}");
    assert!(labels.contains(&"times:"), "parameter labels offered: {labels:?}");
    // Added, not exclusive: an argument slot legitimately takes a value.
    assert!(items.len() > 2, "the general list is still there: {}", items.len());
    let _ = fs::remove_dir_all(&root);
}

/// A label written once is spent -- the list shrinks as the call is filled
/// in, so what is left is exactly what is still missing.
#[test]
fn a_used_parameter_name_is_not_offered_again() {
    let root = temp_root("named_args_used");
    let src = "public void greet(String who, int times) { }
               public void run() { greet(who: \"a\",  }
";
    let (doc, uri) = doc_for(&root, "Named2.jux", src);
    let offset = src.find("\"a\", ").unwrap() + "\"a\", ".len();
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"times:"), "the remaining label: {labels:?}");
    assert!(!labels.contains(&"who:"), "the spent label is gone: {labels:?}");
    let _ = fs::remove_dir_all(&root);
}

   #[test]
fn method_completion_inserts_param_snippet() {
    let root = temp_root("param_snippet");
    let src = "public class Greeter {\n\
                   public String greet(String who, int times) { return who; }\n\
                   public void zero() { }\n\
               }\n\
               public void main() { var g = new Greeter(); g. }\n";
    let (doc, uri) = doc_for(&root, "Greeter.jux", src);
    let offset = src.rfind("g. ").unwrap() + 2;
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);

    let greet = items.iter().find(|i| i.label == "greet").expect("greet offered");
    assert_eq!(greet.insert_text.as_deref(), Some("greet(${1:who}, ${2:times})"));
    assert_eq!(greet.insert_text_format, Some(InsertTextFormat::SNIPPET));
    // The label is the bare name; matching runs on it.
    assert_eq!(greet.filter_text.as_deref(), Some("greet"));

    // No-arg methods insert plain `name()` (caret after the parens).
    let zero = items.iter().find(|i| i.label == "zero").expect("zero offered");
    assert_eq!(zero.insert_text.as_deref(), Some("zero()"));
    assert_eq!(zero.insert_text_format, None);

    let _ = fs::remove_dir_all(&root);
}

/// `private` members are hidden outside their class; `protected` members
/// show for subclasses (and same package) only.
 /// A bare `case |` offers the scrutinee's own variants, qualified the way a
/// pattern must be written.
///
/// `case Color.|` always worked -- that is an ordinary static receiver. The
/// far more useful bare form offered the generic statement bag, leaving the
/// user to remember both the enum's name and its variants, which is exactly
/// what the compiler already knows at that position.
#[test]
fn case_offers_the_scrutinee_variants() {
    let root = temp_root("case_variants");
    let src = "public enum Color { Red, Green, Blue }
               public void run(Color c) {
                   switch (c) {
                       case 
                   }
               }
";
    let (doc, uri) = doc_for(&root, "Case.jux", src);
    let offset = src.find("case ").unwrap() + "case ".len();
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    for v in ["Color.Red", "Color.Green", "Color.Blue"] {
        assert!(labels.contains(&v), "qualified variant `{v}` offered: {labels:?}");
    }
    // Exclusively: a keyword here would push the variants down for nothing.
    assert!(!labels.contains(&"return"), "no statement keywords: {labels:?}");
    let _ = fs::remove_dir_all(&root);
}

/// A `switch` over something with no variants must fall through to the
/// normal completion rather than showing an empty popup.
#[test]
fn case_over_a_non_enum_falls_through() {
    let root = temp_root("case_non_enum");
    let src = "public void run(String s) {
                   switch (s) {
                       case 
                   }
               }
";
    let (doc, uri) = doc_for(&root, "CaseS.jux", src);
    let offset = src.find("case ").unwrap() + "case ".len();
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);
    assert!(!items.is_empty(), "a non-enum scrutinee keeps the general list");
    let _ = fs::remove_dir_all(&root);
}

/// The scrutinee scan must not reach out of its construct: a `case` that is
/// not inside a `switch (...) { }` finds nothing.
 /// Every provider this server advertises, pinned.
///
/// The set was asserted nowhere, so a provider dropped in a refactor would
/// compile, ship, and make the feature stop existing in every editor at
/// once -- silently, because a client simply never asks again for something
/// the server did not advertise. This is the cheapest possible guard
/// against the most expensive kind of regression.
#[test]
fn the_advertised_capabilities_are_the_expected_set() {
    let caps = crate::capabilities::server_capabilities(U16);

    assert!(caps.hover_provider.is_some(), "hover");
    assert!(caps.definition_provider.is_some(), "goto-definition");
    assert!(caps.document_symbol_provider.is_some(), "document symbols");
    assert!(caps.code_action_provider.is_some(), "code actions (auto-import)");
    assert!(caps.references_provider.is_some(), "references");
    assert!(caps.semantic_tokens_provider.is_some(), "semantic tokens");
    assert!(caps.inlay_hint_provider.is_some(), "inlay hints");
    assert!(caps.workspace_symbol_provider.is_some(), "workspace symbols");
    assert_eq!(caps.position_encoding, Some(PositionEncodingKind::UTF16), "the negotiated encoding is echoed");
    match caps.rename_provider {
        Some(OneOf::Right(opts)) => assert_eq!(opts.prepare_provider, Some(true), "prepareRename"),
        other => panic!("rename with a prepare step: {other:?}"),
    }
    match caps.text_document_sync {
        Some(TextDocumentSyncCapability::Options(o)) => {
            assert_eq!(o.change, Some(TextDocumentSyncKind::INCREMENTAL), "incremental sync");
            assert_eq!(o.open_close, Some(true));
        }
        other => panic!("sync options expected: {other:?}"),
    }

    let completion = caps.completion_provider.expect("completion");
    assert_eq!(
        completion.trigger_characters,
        Some(vec![".".to_string(), ":".to_string(), "@".to_string()]),
        "the `@` trigger is what makes annotation completion reachable at all",
    );
    assert_eq!(completion.resolve_provider, Some(true), "lazy doc comments");

    let sig = caps.signature_help_provider.expect("signature help");
    assert_eq!(sig.trigger_characters, Some(vec!["(".to_string(), ",".to_string()]));

    // Not advertised, deliberately: there is no handler, and advertising one
    // would make the editor ask and get nothing.
    assert!(caps.document_formatting_provider.is_none(), "formatting: no handler yet");

    // UTF-8 is echoed when negotiated.
    let utf8 = crate::capabilities::server_capabilities(PositionEncoding::Utf8);
    assert_eq!(utf8.position_encoding, Some(PositionEncodingKind::UTF8));
}

   #[test]
fn case_scan_stays_inside_its_switch() {
    assert_eq!(switch_scrutinee_before("case ", 5), None);
    assert_eq!(switch_scrutinee_before("var x = 1; case ", 16), None);
    // A real one resolves to the byte just past the scrutinee.
    let t = "switch (c) { case ";
    assert_eq!(switch_scrutinee_before(t, t.len()), Some(t.find(')').unwrap()));
}

   #[test]
fn member_visibility_is_enforced() {
    let root = temp_root("visibility");
    fs::write(
        root.join("Base.jux"),
        "package lib;\n\
         public class Base {\n\
             private int secret;\n\
             protected int prot;\n\
             public int open;\n\
         }\n",
    )
    .unwrap();
    let src = "package app;\n\
               import lib.Base;\n\
               public class Sub extends Base {\n\
                   public void m() { this. }\n\
               }\n";
    let (doc, uri) = doc_for(&root, "Sub.jux", src);

    // From inside the subclass (`this.`): protected + public, no private.
    let offset = src.rfind("this. ").unwrap() + "this.".len();
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"open"), "{labels:?}");
    assert!(labels.contains(&"prot"), "protected visible in subclass: {labels:?}");
    assert!(!labels.contains(&"secret"), "private leaked into subclass: {labels:?}");

    // From an unrelated file in another package: public only.
    let src2 = "package other;\n\
                import lib.Base;\n\
                public void use() { var b = new Base(); b. }\n";
    let (doc2, uri2) = doc_for(&root, "Use.jux", src2);
    let offset2 = src2.rfind("b. ").unwrap() + 2;
    let items2 = build_completions(&doc2, &Workspace::default(), &uri2, offset2);
    let labels2: Vec<&str> = items2.iter().map(|i| i.label.as_str()).collect();
    assert!(labels2.contains(&"open"), "{labels2:?}");
    assert!(!labels2.contains(&"prot"), "protected leaked outside hierarchy: {labels2:?}");
    assert!(!labels2.contains(&"secret"), "private leaked: {labels2:?}");

    let _ = fs::remove_dir_all(&root);
}

/// No completions inside strings, comments, or char literals.
 /// `@` is one of the server's trigger characters, so every `@` typed asks
/// this server for completion. With no branch for it the caret fell through
/// to the general bag and the user saw keywords, locals and type names —
/// never an annotation. The IntelliJ plugin has a list of its own but stands
/// down whenever this server is serving, which is the configuration most
/// users run, so `@` effectively had no completion at all.
#[test]
fn at_sign_offers_annotations_only() {
    let root = temp_root("annotations");
    let src = "public class C {\n    @Ov\n    public void m() { }\n}\n";
    let (doc, uri) = doc_for(&root, "A.jux", src);
    let offset = src.find("@Ov").unwrap() + 3;
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();

    assert!(labels.contains(&"Override"), "expected the annotations: {labels:?}");
    assert!(labels.contains(&"Test"), "expected the test hooks: {labels:?}");
    assert!(labels.contains(&"layout"), "expected the layout annotation: {labels:?}");
    // Exclusively: a keyword or a type name after `@` is never legal.
    assert!(
        !labels.contains(&"class") && !labels.contains(&"public"),
        "no keywords after `@`: {labels:?}",
    );
    assert_eq!(
        labels.len(),
        juxc_lex::grammar_spec::BUILTIN_ANNOTATIONS.len(),
        "the list is exactly the compiler's honored set: {labels:?}",
    );
    let _ = fs::remove_dir_all(&root);
}

/// `@` only means an annotation when it is directly against the name —
/// `@ Test` is not annotation syntax, and treating a stray `@` as one would
/// replace the normal completion list.
#[test]
fn a_detached_at_sign_is_not_an_annotation() {
    assert!(at_sign_before("@Over", 1));
    assert!(!at_sign_before("@ Over", 2));
    assert!(!at_sign_before("Over", 0));
    assert!(!at_sign_before("a.Over", 2));
}

   #[test]
fn no_completions_inside_strings_or_comments() {
    assert_eq!(analyze_context("var s = \"hel").1, ScanMode::Str { interp: false });
    assert_eq!(analyze_context("/* doc ").1, ScanMode::BlockComment);
    assert_eq!(analyze_context("// note").1, ScanMode::LineComment);
    assert_eq!(analyze_context("var c = 'x").1, ScanMode::Char);
    assert_eq!(analyze_context("public void m() {").1, ScanMode::Code);
    // A `$"…${hole}…"` interpolation hole IS code again.
    assert_eq!(analyze_context("var s = $\"v=${").1, ScanMode::Code);
    // …and after the hole closes we're back inside the string.
    assert_eq!(
        analyze_context("var s = $\"v=${x}").1,
        ScanMode::Str { interp: true }
    );
    // Raw strings: embedded `"` and `\` are content; only `"""` closes.
    assert_eq!(
        analyze_context("var s = \"\"\"say \" loud").1,
        ScanMode::RawStr { interp: false }
    );
    assert_eq!(analyze_context("var s = \"\"\"a \" b\"\"\"; var t = ").1, ScanMode::Code);
    // A trailing backslash is raw content, never an escape of the closer.
    assert_eq!(analyze_context("var s = \"\"\"C:\\\"\"\"; var t = ").1, ScanMode::Code);
    // Raw interpolated strings get code holes too.
    assert_eq!(analyze_context("var s = $\"\"\"v=${").1, ScanMode::Code);
    // A mid-edit unclosed plain string resyncs at end-of-line (the lexer
    // terminates it there) instead of swallowing the rest of the file.
    assert_eq!(analyze_context("var s = \"oops\nvar t = ").1, ScanMode::Code);

    // End-to-end: a caret inside a string yields zero items.
    let root = temp_root("string_silence");
    let src = "public void main() { var s = \"hello wo\"; }\n";
    let (doc, uri) = doc_for(&root, "S.jux", src);
    let offset = src.find("wo\"").unwrap() + 2; // inside the literal
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);
    assert!(items.is_empty(), "no completions inside a string literal");
    let _ = fs::remove_dir_all(&root);
}

/// Type-position detection: `extends` vs `implements` vs `new` vs
/// generic bounds.
#[test]
fn type_position_detection() {
    let t = "public class Foo extends ";
    assert_eq!(
        type_position(t, t.len()),
        Some(TypePos::Extends { interfaces: false, generic: false })
    );
    let t2 = "public interface I extends ";
    assert_eq!(
        type_position(t2, t2.len()),
        Some(TypePos::Extends { interfaces: true, generic: false })
    );
    let t3 = "public class C implements A, ";
    assert_eq!(type_position(t3, t3.len()), Some(TypePos::Implements));
    let t4 = "public class C<T extends ";
    assert_eq!(
        type_position(t4, t4.len()),
        Some(TypePos::Extends { interfaces: false, generic: true })
    );
    let t5 = "var x = new ";
    assert_eq!(type_position(t5, t5.len()), Some(TypePos::New));
    // Plain statement position is NOT a type position.
    let t6 = "public void m() { var x = ";
    assert_eq!(type_position(t6, t6.len()), None);
    // After a COMPLETE parent type + space, the next word is
    // `implements`/`{` — no longer a type position.
    let t7 = "public class C extends Base ";
    assert_eq!(type_position(t7, t7.len()), None);
    // …but after a comma the list is still open.
    let t8 = "public class C implements A, ";
    assert_eq!(type_position(t8, t8.len()), Some(TypePos::Implements));
    // Keywords inside comments never fake a type position.
    let t9 = "public void m() {\n    // extends Base,\n    ";
    assert_eq!(type_position(t9, t9.len()), None);
    let t10 = "public void m() {\n    // new\n    ";
    assert_eq!(type_position(t10, t10.len()), None);
}

/// The brace header survives a trailing line comment, so a next-line `{`
/// still classifies as a type body.
#[test]
fn line_comment_preserves_brace_header() {
    let (ctx, mode) = analyze_context("public class Foo // widget\n{\n    ");
    assert_eq!(mode, ScanMode::Code);
    assert_eq!(ctx, CtxKind::TypeBody);
}

/// `import_prefix` only sees the text after the line's last `;`.
#[test]
fn import_prefix_respects_statement_boundaries() {
    assert_eq!(import_prefix("import xss.it.", 14).as_deref(), Some("xss.it."));
    // After the `;` the line is ordinary code, not import-path mode.
    assert_eq!(import_prefix("import a.B; foo", 15), None);
    // …and a second `import` after a `;` IS path mode again.
    assert_eq!(import_prefix("import a.B; import c.", 21).as_deref(), Some("c."));
    // Not an import line at all.
    assert_eq!(import_prefix("var x = 1", 9), None);
}

/// `extends` offers only extendable classes; `implements` only interfaces;
/// `new` only instantiable types.
#[test]
fn type_positions_filter_candidates() {
    let root = temp_root("type_positions");
    let src = "public final class Locked { }\n\
               public abstract class Abs { }\n\
               public class Open { }\n\
               public interface Shape { }\n\
               public class Next extends ";
    let (doc, uri) = doc_for(&root, "T.jux", src);
    let ws = Workspace::default();

    // `extends` — Open yes; Locked (final), Shape (interface) no.
    let items = build_completions(&doc, &ws, &uri, src.len());
    let labels: Vec<&str> = items.iter().map(|i| i.label.as_str()).collect();
    assert!(labels.contains(&"Open"), "{labels:?}");
    assert!(labels.contains(&"Abs"), "abstract classes are extendable: {labels:?}");
    assert!(!labels.contains(&"Locked"), "final class offered for extends: {labels:?}");
    assert!(!labels.contains(&"Shape"), "interface offered for class extends: {labels:?}");
    // No keywords/snippets in a type-only position.
    assert!(!labels.contains(&"while") && !labels.contains(&"public"), "{labels:?}");

    // `implements` — interfaces only.
    let src2 = format!("{}Open implements ", &src[..src.len() - "extends ".len()]);
    let offset2 = src2.len();
    let doc2 = with_text(&doc, &src2);
    let items2 = build_completions(&doc2, &ws, &uri, offset2);
    let labels2: Vec<&str> = items2.iter().map(|i| i.label.as_str()).collect();
    assert!(labels2.contains(&"Shape"), "{labels2:?}");
    assert!(!labels2.contains(&"Open"), "class offered for implements: {labels2:?}");

    // `new` — concrete classes, not abstract ones or interfaces.
    let src3 = "public abstract class Abs { }\npublic class Open { }\npublic interface Shape { }\npublic void m() { var x = new ";
    let (doc3, uri3) = doc_for(&root, "N.jux", src3);
    let items3 = build_completions(&doc3, &ws, &uri3, src3.len());
    let labels3: Vec<&str> = items3.iter().map(|i| i.label.as_str()).collect();
    assert!(labels3.contains(&"Open"), "{labels3:?}");
    assert!(!labels3.contains(&"Abs"), "abstract class offered for new: {labels3:?}");
    assert!(!labels3.contains(&"Shape"), "interface offered for new: {labels3:?}");

    let _ = fs::remove_dir_all(&root);
}

/// `import …` lines complete package segments first, then type names —
/// and nothing else.
#[test]
fn import_completion_offers_package_segments_then_types() {
    let root = temp_root("import_completion");
    fs::write(
        root.join("Widget.jux"),
        "package xss.it;\npublic class Widget { }\n",
    )
    .unwrap();
    let src = "import xss.\n";
    let (doc, uri) = doc_for(&root, "main.jux", src);
    let offset = src.find("xss.").unwrap() + "xss.".len();
    let items = build_completions(&doc, &Workspace::default(), &uri, offset);
    let it = items.iter().find(|i| i.label == "it").expect("package segment offered");
    assert_eq!(it.kind, Some(CompletionItemKind::MODULE));
    // No keywords/snippets on an import line.
    assert!(items.iter().all(|i| i.kind != Some(CompletionItemKind::KEYWORD)));

    // One segment deeper: the terminal type name, with its FQN as detail.
    let src2 = "import xss.it.\n";
    let doc2 = with_text(&doc, src2);
    let offset2 = src2.find("it.").unwrap() + "it.".len();
    let items2 = build_completions(&doc2, &Workspace::default(), &uri, offset2);
    let widget = items2.iter().find(|i| i.label == "Widget").expect("type offered");
    assert_eq!(widget.kind, Some(CompletionItemKind::CLASS));
    assert_eq!(widget.detail.as_deref(), Some("xss.it.Widget"));

    let _ = fs::remove_dir_all(&root);
}

/// The resolve pipeline: a member item's `data` locates the declaration
/// and reads its doc comment — here exercised through the same helpers the
/// `completion_resolve` handler calls.
#[test]
fn completion_resolve_locates_member_doc_comment() {
    let root = temp_root("resolve_doc");
    let src = "public class Doc {\n\
                   /// Adds things together.\n\
                   public int add(int a) { return a; }\n\
               }\n";
    let (doc, _uri) = doc_for(&root, "Doc.jux", src);
    let span = member_decl_span(&doc.symbols, "Doc", "add").expect("member span");
    let line = doc_comment_before(src, span.start as usize);
    assert_eq!(line.as_deref(), Some("Adds things together."));

    let _ = fs::remove_dir_all(&root);
}
