//! The crate-boundary sweep (ERRATA E1XX-SWEEPB), on one crate in naga's
//! shape: several items of one simple name in different modules, a type
//! whose `From` sources share a simple name, and error types a Jux program
//! sees as the closest Jux exception.
//!
//! The fixture is REAL rustdoc JSON: `fixtures/sweepb-src/lib.rs` documented
//! with `cargo +nightly rustdoc -- -Z unstable-options --output-format json`.
//!
//! Regenerating: copy `sweepb-src/lib.rs` into a scratch crate named `sbfix`,
//! run the command above, and copy `target/doc/sbfix.json` over
//! `sweepb.rustdoc.json`.

/// The stub for `rust.sbfix`, generated as the driver generates a bound
/// crate's (a family of one).
fn stub_file() -> juxc_bindgen::StubFile {
    let json = include_str!("fixtures/sweepb.rustdoc.json");
    let jsons = [("sbfix", json)];
    let fam = juxc_bindgen::family::FamilyPaths::read("sbfix", &jsons).expect("the crate reads");
    juxc_bindgen::ingest::generate_family(&jsons, "rust.sbfix", &fam, &std::collections::HashMap::new())
        .expect("the crate ingests")
}

/// The rendered host stub.
fn host() -> String {
    juxc_bindgen::render_stub(&stub_file())
}

/// The rendered nested package `package`.
fn nested(package: &str) -> String {
    let stub = stub_file();
    let file = stub.nested.iter().find(|n| n.package == package).unwrap_or_else(|| {
        panic!("no nested package `{package}`: {:?}", stub.nested.iter().map(|n| &n.package).collect::<Vec<_>>())
    });
    juxc_bindgen::render_stub(file)
}

/// The declaration of `head` (`class Error`) with its annotation block.
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

/// §G.4.1: every type of a shared simple name is declared, each in the nested
/// package its module path names, with its own methods; the most general one
/// keeps the plain name, and is reachable by its module path too.
#[test]
fn every_type_of_a_shared_name_is_declared_in_its_modules_package() {
    let wgsl = nested("rust.sbfix.front.wgsl");
    assert_has(&wgsl, "package rust.sbfix.front.wgsl;");
    assert_has(&wgsl, "import rust.sbfix.*;");
    let error = decl(&wgsl, "class Error");
    assert_has(&error, "@rust(\"sbfix::front::wgsl::Error\")");
    assert_has(&error, "public u32 line();");
    assert_has(&decl(&nested("rust.sbfix.front.spv"), "enum Error"), "BadMagic(u32), Truncated");
    assert_has(&decl(&nested("rust.sbfix.back.spv"), "enum Error"), "@rust(\"sbfix::back::spv::Error\")");
    // The trait `de::Error` has the shortest path, so it keeps `rust.sbfix.Error`,
    // and `rust.sbfix.de.Error` names it as well.
    assert_has(&decl(&host(), "interface Error"), "@rust(\"sbfix::de::Error\")");
    assert_has(&nested("rust.sbfix.de"), "public type Error = rust.sbfix.Error;");
}

/// Every signature names the exact `Error` it takes or throws, never the bare
/// name, which would mean the trait that kept it.
#[test]
fn every_signature_names_the_exact_error() {
    let host = host();
    let module = decl(&host, "class Module");
    assert_has(&module, "public static String describe_parse_error(&rust.sbfix.front.wgsl.Error error);");
    assert_has(&module, "public static bool is_capability_error(&rust.sbfix.back.spv.Error error);");
    assert_has(
        &host,
        "public String write(&Module module, &String entry, &String capability) throws rust.sbfix.back.spv.Error;",
    );
    assert_has(&nested("rust.sbfix.front.wgsl"), "public Module parse(&String source) throws rust.sbfix.front.wgsl.Error;");
    for text in [host.clone(), nested("rust.sbfix.front.wgsl"), nested("rust.sbfix.front.spv"), nested("rust.sbfix.back.spv")] {
        for line in text.lines().filter(|l| l.contains('(') && !l.trim_start().starts_with('@')) {
            assert!(
                !line.contains(" Error ") && !line.contains("(Error") && !line.contains("throws Error;"),
                "a bare `Error` in a signature: {line}"
            );
        }
    }
}

/// Two free functions of one name are each declared in their module's nested
/// package; the most general keeps the plain name in the host's package.
#[test]
fn same_name_functions_are_declared_in_their_modules_packages() {
    assert_has(&nested("rust.sbfix.front.spv"), "@rust(\"sbfix::front::spv::parse\")\npublic Module parse(Vec<u32> words)");
    assert_has(&nested("rust.sbfix.front.wgsl"), "@rust(\"sbfix::front::wgsl::parse\")\npublic Module parse(&String source)");
    assert_has(&host(), "@rust(\"sbfix::front::spv::parse\")\npublic Module parse(Vec<u32> words)");
}

/// A type of Rust's own library whose simple name the crate declares is
/// written `rust.std.<Name>`: `io::Error` inside a crate with `Error`s of its
/// own is not any of them.
#[test]
fn a_std_type_of_a_name_the_crate_declares_is_written_through_rust_std() {
    assert_has(&decl(&nested("rust.sbfix.back.spv"), "enum Error"), "Io(rust.std.Error)");
    assert_has(&decl(&host(), "class ConfigError"), "@RustFrom(\"rust.std.Error\")");
}

/// The names inside the string markers and the `implements` clauses are
/// qualified the way signatures are: `Diagnostic` converts from both front
/// ends' errors, each named exactly, and a type implementing the crate's own
/// `de::Error` trait says which `Error`. `std::error::Error` is not the crate's
/// trait of that name, so no type claims to implement it.
#[test]
fn marker_and_implements_names_are_qualified_like_signatures() {
    let host = host();
    assert_has(
        &decl(&host, "class Diagnostic"),
        "@RustFrom(\"rust.sbfix.front.spv.Error,rust.sbfix.front.wgsl.Error\")",
    );
    assert_has(&decl(&host, "class DataError"), "public class DataError implements rust.sbfix.Error {");
    for head in ["class ConfigError", "class Refusal", "enum LookupError", "class Elapsed"] {
        assert!(!decl(&host, head).contains("implements"), "{head} implements only std's `Error`:\n{}", decl(&host, head));
    }
}

/// What each error type is to a Jux program, read off its shape.
#[test]
fn error_types_carry_the_jux_exception_their_shape_says() {
    let host = host();
    let shape = |head: &str, class: &str, path: &str| {
        let d = decl(&host, head);
        assert_has(&d, &format!("@RustError(\"{class}\")"));
        assert_has(&d, &format!("@RustTypeName(\"{path}\")"));
    };
    shape("class ConfigError", "IOException", "sbfix::ConfigError");
    shape("class DataError", "FormatException", "sbfix::DataError");
    shape("class VersionError", "IllegalArgumentException", "sbfix::VersionError");
    shape("class DecimalError", "NumberFormatException", "sbfix::DecimalError");
    shape("class Elapsed", "TimeoutException", "sbfix::Elapsed");
    let lookup = decl(&host, "enum LookupError");
    assert_has(&lookup, "@RustError(\"\")");
    assert_has(&lookup, "@RustErrorVariants(\"KeyNotFound:NoSuchElementException\")");
    let writer = decl(&nested("rust.sbfix.back.spv"), "enum Error");
    assert_has(&writer, "@RustError(\"IOException\")");
    assert_has(&writer, "@RustTypeName(\"sbfix::back::spv::Error\")");
    assert_has(
        &writer,
        "@RustErrorVariants(\"UnsupportedCapability:UnsupportedOperationException;EntryPointNotFound:NoSuchElementException;TimedOut:TimeoutException\")",
    );
    // Nothing about these says more than "an error": a `LibraryException`.
    assert!(!decl(&host, "class Refusal").contains("@RustError"), "{}", decl(&host, "class Refusal"));
    assert!(!decl(&nested("rust.sbfix.front.wgsl"), "class Error").contains("@RustError"));
}
