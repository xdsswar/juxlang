//! Associated functions and constants a type has only through a trait impl
//! (ERRATA E151): the crate's own trait's, and the standard
//! library's construction and conversion traits', written onto the type's
//! stub as statics marked with the trait they come from.
//!
//! The fixture is REAL rustdoc JSON: `fixtures/sweepc3-src/lib.rs` documented
//! with `cargo +nightly rustdoc -- -Z unstable-options --output-format json`.
//!
//! Regenerating: copy `sweepc3-src/lib.rs` into a scratch crate named
//! `c3fix`, run the command above, and copy `target/doc/c3fix.json` over
//! `sweepc3.rustdoc.json`.

fn stub() -> String {
    let json = include_str!("fixtures/sweepc3.rustdoc.json");
    let file = juxc_bindgen::ingest::generate_from_json(json, "rust.c3fix").expect("the crate ingests");
    juxc_bindgen::render_stub(&file)
}

/// The class `name`'s body.
fn class(stub: &str, name: &str) -> String {
    let at = stub.find(&format!("public class {name} ")).unwrap_or_else(|| panic!("no class `{name}`:\n{stub}"));
    let to = stub[at..].find("\n}\n").map_or(stub.len(), |i| at + i + 3);
    stub[at..to].to_string()
}

#[test]
fn a_trait_impls_statics_are_the_types_statics_marked_with_the_trait() {
    let stub = stub();
    let clock = class(&stub, "Clock");
    for want in [
        // The crate's own trait: its associated function and constant.
        "@RustTrait(\"c3fix::Tick\") public static final u32 STEP;",
        "@RustTrait(\"c3fix::Tick\") public static Clock origin();",
        // The standard library's, by their public paths.
        "@RustTrait(\"std::default::Default\") public static Clock default();",
        "@RustTrait(\"std::str::FromStr\") public static Clock from_str(&String s) throws ParseIntError;",
        "@RustTrait(\"std::convert::From<_>\") public static Clock from(u32 t);",
        "@RustTrait(\"std::convert::From<_>\") public static Clock from(bool b);",
        // The inherent members are as before.
        "public static Clock at(u32 t);",
        "public u32 now();",
    ] {
        assert!(clock.contains(want), "expected `{want}` in:\n{clock}");
    }
    // An instance method of a trait is not a static of the type.
    assert!(!clock.contains("static Clock next"), "{clock}");
}

/// `cargo test -p juxc-bindgen --test sweepc3_fixture print_stub -- --ignored
/// --nocapture` shows the whole stub, which `bin/juxc/tests/trait_statics.rs`
/// uses as its crate's.
#[test]
#[ignore]
fn print_stub() {
    println!("{}", stub());
}

/// Nothing of a trait's machinery is a member: not a marker trait's, not an
/// auto trait's, not a blanket impl's (`From<T> for T`, `Into`, `Any`).
#[test]
fn only_the_nameable_construction_traits_contribute() {
    let clock = class(&stub(), "Clock");
    for unwanted in ["type_id", "into(", "borrow(", "try_into", "clone_to_uninit", "UnwindSafe"] {
        assert!(!clock.contains(unwanted), "`{unwanted}` leaked into:\n{clock}");
    }
    let traits: Vec<&str> = clock
        .lines()
        .filter_map(|l| l.trim().strip_prefix("@RustTrait(\""))
        .filter_map(|l| l.split('"').next())
        .collect();
    for t in &traits {
        assert!(
            matches!(*t, "c3fix::Tick" | "std::default::Default" | "std::str::FromStr" | "std::convert::From<_>"),
            "unexpected trait `{t}`"
        );
    }
}
