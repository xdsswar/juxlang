//! Bindgen type mapping for the shapes the leaker app ran into (LEAKS L2-L21,
//! ERRATA E1XX-GAP31, Bindgen §G.3.6, §G.3.7, §G.6.6).
//!
//! The fixture is REAL rustdoc JSON: `fixtures/leaks-src/lib.rs` documented
//! with `cargo +nightly rustdoc -- -Z unstable-options --output-format json`.
//! Each shape is a few lines of that crate, so no heavy crate is needed to pin
//! what egui, printpdf, lopdf and genpdf needed.
//!
//! Regenerating: copy `leaks-src/lib.rs` into a scratch crate named `g31fix`,
//! run the command above, and copy `target/doc/g31fix.json` over
//! `leaks.rustdoc.json`.

fn stub() -> String {
    let json = include_str!("fixtures/leaks.rustdoc.json");
    let file = juxc_bindgen::ingest::generate_from_json(json, "rust.g31fix")
        .expect("the fixture ingests");
    juxc_bindgen::render_stub(&file)
}

/// The declaration of `head` (`class Panel`) with its annotation block.
fn decl(stub: &str, head: &str) -> String {
    let at = stub
        .find(&format!("public {head} "))
        .unwrap_or_else(|| panic!("no `{head}` in the stub:\n{stub}"));
    let from = stub[..at].rfind("\n\n").map_or(0, |i| i + 2);
    let to = stub[at..].find("\n}\n").map_or(stub.len(), |i| at + i + 3);
    stub[from..to].to_string()
}

fn assert_has(text: &str, want: &str) {
    assert!(text.contains(want), "expected `{want}` in:\n{text}");
}

/// L2: `impl Into<Label>` is a generic slot typed by its target, and the
/// target records what converts into it. A borrowed `&str` source folds into
/// the owned `String` one.
#[test]
fn impl_into_is_a_generic_slot_and_the_target_lists_its_sources() {
    let s = stub();
    assert_has(&decl(&s, "class Panel"), "public void label(@RustImpl Label text);");
    assert_has(&decl(&s, "class Label"), "@RustFrom(\"Rich,String\")");
}

/// L3: an `impl Trait` and a `&mut dyn Trait` parameter are generic slots
/// too, never a boxed trait object.
#[test]
fn impl_trait_and_dyn_trait_are_generic_slots() {
    let s = stub();
    let panel = decl(&s, "class Panel");
    assert_has(&panel, "public bool add(@RustImpl Widget w);");
    assert_has(&panel, "public void edit(@RustImpl &mut Buffer b);");
}

/// L3/L4/L13: a trait's blanket impls and its impls for another crate's type
/// are recorded on the trait.
#[test]
fn blanket_and_foreign_impls_are_recorded() {
    let s = stub();
    assert_has(&decl(&s, "interface IntoAtom"), "@RustBlanket(\"Into<Label>\")");
    assert_has(&decl(&s, "interface AsKey"), "@RustBlanket(\"Hash + Debug\")");
    assert_has(&decl(&s, "interface Buffer"), "@RustImplementedBy(\"String\")");
    assert_has(&decl(&s, "class Label"), "@RustHash");
}

/// L11/L16: an erased `Arc` is marked where a value goes in, on a parameter
/// and on a variant payload, and where one comes back out.
#[test]
fn erased_shared_pointers_are_marked() {
    let s = stub();
    let panel = decl(&s, "class Panel");
    assert_has(&panel, "public void set_style(@RustArc Style s);");
    assert_has(&panel, "@RustDerefOut public Style style();");
    assert_has(&decl(&s, "enum Text"), "Shared(@RustArc Rich)");
}

/// L17: associated constants are `static final` fields.
#[test]
fn associated_constants_are_static_final_fields() {
    assert_has(&decl(&stub(), "class Color"), "public static final Color RED;");
}

/// L19: a tuple struct has its positional constructor and fields; a variant
/// with named fields keeps them, and records their names.
#[test]
fn tuple_structs_and_named_field_variants_are_constructible() {
    let s = stub();
    let mm = decl(&s, "class Mm");
    assert_has(&mm, "@RustTuple");
    assert_has(&mm, "public float _0;");
    assert_has(&mm, "@RustTuple public Mm(float _0);");
    let op = decl(&s, "enum Op");
    assert_has(&op, "@RustStructVariants(\"Fill:col;Move:x,y\")");
    assert_has(&op, "Fill(Color), Move(Mm, Mm), End");
}

/// L19: maps keep their own names, so a program can build the value.
#[test]
fn maps_keep_their_names() {
    assert_has(
        &decl(&stub(), "class Panel"),
        "public void maps(&BTreeMap<String, ubyte> a, HashMap<String, ubyte> b);",
    );
}

/// L21: `Extend` alone does not make a collection; a collection also
/// iterates.
#[test]
fn a_type_that_only_extends_itself_is_not_a_collection() {
    let s = stub();
    assert!(!decl(&s, "class Style").contains("@RustCollection"), "{}", decl(&s, "class Style"));
    assert_has(&decl(&s, "class Bag"), "@RustCollection");
}

/// L20: a keyword-named field is written as it is (the parser reads it).
#[test]
fn a_keyword_named_field_is_kept() {
    assert_has(&decl(&stub(), "struct Operation"), "public String operator;");
}

/// Gap 30's follow-up: which closure arguments a crate lends read-only.
#[test]
fn closure_arguments_lent_read_only_are_recorded() {
    let panel = decl(&stub(), "class Panel");
    assert_has(&panel, "@RustClosureShared(\"0:0\") public R read<R>");
    let write = panel.lines().find(|l| l.contains(" write<R>")).expect("write is declared");
    assert!(!write.contains("RustClosureShared"), "a `&mut` argument is not read-only: {write}");
}
