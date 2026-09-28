//! Bindgen type mapping for the shapes the leaker app ran into (LEAKS L2-L21,
//! ERRATA E128, Bindgen §G.3.6, §G.3.7, §G.6.6).
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

// ---------------------------------------------------------------------------
// Gap 37: a crate family whose members share a simple name (LEAKS L29), and a
// crate's deprecated items (LEAKS L39). The fixture is two crates in eframe's
// and egui's shape, `fixtures/family-src/{g37app,g37ui}/lib.rs`, documented
// the same way (a scratch workspace with `g37ui` and `g37app` depending on it
// by path; copy `target/doc/{g37app,g37ui}.json` over the fixtures).
// ---------------------------------------------------------------------------

/// The family stub for `rust.g37app`, and the program binding `excluded`
/// crates in their own right, as the driver generates it.
fn family_stub(excluded: &[(&str, &str)]) -> juxc_bindgen::StubFile {
    let host = include_str!("fixtures/g37app.rustdoc.json");
    let member = include_str!("fixtures/g37ui.rustdoc.json");
    let jsons = [("g37app", host), ("g37ui", member)];
    let fam = juxc_bindgen::family::FamilyPaths::read("g37app", &jsons).expect("the family reads");
    let excluded: std::collections::HashMap<String, String> =
        excluded.iter().map(|(c, p)| (c.to_string(), p.to_string())).collect();
    let mut stub = juxc_bindgen::ingest::generate_family(&jsons, "rust.g37app", &fam, &excluded)
        .expect("the family ingests");
    juxc_bindgen::ingest::rewrite_reexported_paths(&mut stub, "g37app", host).expect("re-exports read");
    stub
}

/// The nested package `package` of a family stub.
fn nested<'a>(stub: &'a juxc_bindgen::StubFile, package: &str) -> &'a juxc_bindgen::StubFile {
    stub.nested.iter().find(|n| n.package == package).unwrap_or_else(|| {
        panic!("no nested package `{package}`: {:?}", stub.nested.iter().map(|n| &n.package).collect::<Vec<_>>())
    })
}

/// L29: the host keeps its own `Frame` under the plain name, and the member's
/// is DECLARED in the nested package its Rust path names, with its methods.
#[test]
fn both_types_of_a_shared_name_are_declared() {
    let stub = family_stub(&[]);
    let host = juxc_bindgen::render_stub(&stub);
    let host_frame = decl(&host, "class Frame");
    assert_has(&host_frame, "@rust(\"g37app::Frame\")");
    assert_has(&host_frame, "public void set_title(&String title);");
    let member = juxc_bindgen::render_stub(nested(&stub, "rust.g37app.g37ui"));
    assert_has(&member, "package rust.g37app.g37ui;");
    assert_has(&member, "import rust.g37app.*;");
    let member_frame = decl(&member, "class Frame");
    assert_has(&member_frame, "@rust(\"g37app::g37ui::containers::frame::Frame\")");
    assert_has(&member_frame, "public rust.g37app.g37ui.Frame fill(Color fill);");
    assert_has(&member_frame, "public String describe();");
}

/// L29: every signature names the exact `Frame` it takes, in the member's
/// types and in the host's own functions alike.
#[test]
fn every_signature_names_the_exact_type() {
    let host = juxc_bindgen::render_stub(&family_stub(&[]));
    assert_has(&decl(&host, "class Panel"), "public Panel frame(rust.g37app.g37ui.Frame frame);");
    assert_has(
        &decl(&host, "class Ui"),
        "public R drop_zone<R>(rust.g37app.g37ui.Frame frame, (Ui) -> R add_contents);",
    );
    assert_has(&host, "public rust.g37app.g37ui.Frame default_panel_frame();");
    assert_has(&host, "public String run((Ui, rust.g37app.Frame) -> void app);");
    // No signature is left saying a bare `Frame`.
    for line in host.lines().filter(|l| l.contains('(')) {
        assert!(
            !line.contains(" Frame ") && !line.contains("(Frame ") && !line.contains(", Frame)"),
            "a bare `Frame` in a signature: {line}"
        );
    }
}

/// L29: the nested package mirrors the member's public path, so its other
/// types are reachable there too, as aliases of their one declaration.
#[test]
fn a_nested_package_aliases_the_members_other_types() {
    let stub = family_stub(&[]);
    let member = juxc_bindgen::render_stub(nested(&stub, "rust.g37app.g37ui"));
    assert_has(&member, "public type Ui = rust.g37app.Ui;");
    assert_has(&member, "public type Panel = rust.g37app.Panel;");
    assert_has(&member, "public type Color = rust.g37app.Color;");
    assert!(!member.contains("public type Frame"), "the member's `Frame` is declared, not aliased:\n{member}");
    // The host's own items live in the host's package only.
    assert_eq!(stub.nested.len(), 1, "{:?}", stub.nested.iter().map(|n| &n.package).collect::<Vec<_>>());
}

/// L29 with the member bound in its own right (`rust.g37ui` beside
/// `rust.g37app`): the member's `Frame` is that package's, and the nested
/// package aliases it rather than declaring a second copy.
#[test]
fn a_member_bound_in_its_own_right_is_referred_to_by_its_own_package() {
    let stub = family_stub(&[("g37ui", "rust.g37ui")]);
    let host = juxc_bindgen::render_stub(&stub);
    assert_has(&decl(&host, "class Frame"), "@rust(\"g37app::Frame\")");
    assert_has(&host, "public rust.g37ui.Frame default_panel_frame();");
    let member = juxc_bindgen::render_stub(nested(&stub, "rust.g37app.g37ui"));
    assert_has(&member, "public type Frame = rust.g37ui.Frame;");
    assert_has(&member, "public type Ui = rust.g37ui.Ui;");
}

/// L39: a deprecated method carries `@Deprecated` with the crate's note.
#[test]
fn a_deprecated_method_carries_the_crates_note() {
    let host = juxc_bindgen::render_stub(&family_stub(&[]));
    let panel = decl(&host, "class Panel");
    assert_has(
        &panel,
        "@Deprecated(message = \"Renamed to `show`\") @RustClosureRefs(\"1\") public R show_inside<R>",
    );
    let show = panel.lines().find(|l| l.contains(" show<R>")).expect("`show` is declared");
    assert!(!show.contains("@Deprecated"), "`show` is not deprecated: {show}");
}
