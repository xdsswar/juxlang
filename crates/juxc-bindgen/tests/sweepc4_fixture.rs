//! Blanket impls and `FromIterator` (ERRATA E152): a blanket impl of
//! one of the crate's own traits gives its associated functions and
//! constants to every type of the stub its bound is KNOWN to cover, from the
//! stub's recorded facts, and to no other; `FromIterator<A>` is a static
//! `from_iter(Vec<A>)`.
//!
//! The fixture is REAL rustdoc JSON: `fixtures/sweepc4-src/lib.rs` documented
//! with `cargo +nightly rustdoc -- -Z unstable-options --output-format json`.
//!
//! Regenerating: copy `sweepc4-src/lib.rs` into a scratch crate named
//! `c4fix`, run the command above, and copy `target/doc/c4fix.json` over
//! `sweepc4.rustdoc.json`.

fn stub() -> String {
    let json = include_str!("fixtures/sweepc4.rustdoc.json");
    let file = juxc_bindgen::ingest::generate_from_json(json, "rust.c4fix").expect("the crate ingests");
    juxc_bindgen::render_stub(&file)
}

fn class(stub: &str, name: &str) -> String {
    let at = stub.find(&format!("public class {name} ")).unwrap_or_else(|| panic!("no class `{name}`:\n{stub}"));
    let to = stub[at..].find("\n}\n").map_or(stub.len(), |i| at + i + 3);
    stub[at..to].to_string()
}

#[test]
fn a_blanket_impl_reaches_the_types_its_bound_covers() {
    let stub = stub();
    let clock = class(&stub, "Clock");
    for want in [
        // `impl<T: Default + Clone> Maker for T`: Clock is both.
        "@RustTrait(\"c4fix::Maker\") public static final u32 KIND;",
        "@RustTrait(\"c4fix::Maker\") public static Clock make_default();",
        // `impl<T: Named> Labelled for T`: Clock implements Named.
        "@RustTrait(\"c4fix::Labelled\") public static String label();",
        // `FromIterator<u32>`: any collection of `u32`, as a `Vec`.
        "@RustTrait(\"std::iter::FromIterator<_>\") public static Clock from_iter(Vec<u32> iter);",
    ] {
        assert!(clock.contains(want), "expected `{want}` in:\n{clock}");
    }
    // `impl<T: Send> Portable for T`: `Send` is nothing a stub records, so
    // the impl covers no type, rather than every type by guess.
    assert!(!clock.contains("portable"), "{clock}");
}

#[test]
fn a_type_the_bound_does_not_cover_gets_nothing() {
    let stub = stub();
    let plain = stub
        .find("class Plain")
        .or_else(|| stub.find("struct Plain"))
        .map(|at| &stub[at..at + stub[at..].find("\n}\n").unwrap_or(stub.len() - at)])
        .unwrap_or_else(|| panic!("no Plain:\n{stub}"));
    for unwanted in ["make_default", "KIND", "label", "portable", "RustTrait"] {
        assert!(!plain.contains(unwanted), "`{unwanted}` on Plain:\n{plain}");
    }
}

/// `cargo test -p juxc-bindgen --test sweepc4_fixture print_stub -- --ignored
/// --nocapture` shows the whole stub, which `bin/juxc/tests/trait_statics.rs`
/// uses as its second crate's.
#[test]
#[ignore]
fn print_stub() {
    println!("{}", stub());
}
