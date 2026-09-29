//! Statics a foreign type has only through a trait impl (ERRATA
//! E151): a crate trait's associated function and constant, and the
//! standard library's `Default`, `From` and `FromStr`, called on the type the
//! way a Jux program calls any static, and lowered through the trait,
//! `<c3fix::Clock as c3fix::Tick>::origin()`.
//!
//! The crate, `c3fix`, is written here (its source is the bindgen fixture's,
//! `crates/juxc-bindgen/tests/fixtures/sweepc3-src/lib.rs`), and its stub is
//! the one bindgen writes for it (`sweepc3_fixture.rs` holds bindgen to it).
//! The program is BUILT and RUN against the crate: a call through the wrong
//! trait path, or an unopened `Result`, is a rustc error or a wrong answer.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

const C3FIX_LIB: &str = include_str!("../../../crates/juxc-bindgen/tests/fixtures/sweepc3-src/lib.rs");

/// What bindgen writes for `c3fix` (see `sweepc3_fixture.rs`).
const C3FIX_STUB: &str = r#"package rust.c3fix;

/** A counter. */
@rust("c3fix::Clock")
@RustClone
@RustDebug
@RustPartialEq
@RustDefault
@RustFrom("bool,u32")
public class Clock implements Tick {
    @RustTrait("c3fix::Tick") public static final u32 STEP;
    @RustDefault public Clock();
    public static Clock at(u32 t);
    public u32 now();
    @RustTrait("std::default::Default") public static Clock default();
    @RustTrait("c3fix::Tick") public static Clock origin();
    @RustTrait("std::str::FromStr") public static Clock from_str(&String s) throws ParseIntError;
    @RustTrait("std::convert::From<_>") public static Clock from(u32 t);
    @RustTrait("std::convert::From<_>") public static Clock from(bool b);
}

/** A crate trait with an associated function and an associated constant. */
@rust("c3fix::Tick")
public interface Tick {
    @RustStatic public Self origin();
    public Self next();
}
"#;

const PROGRAM: &str = r#"import rust.c3fix.Clock;

void main() {
    // The crate's own trait: an associated function and a constant.
    var o = Clock.origin();
    print(o.now());
    print(Clock.STEP);
    // The standard library's, on the crate's type.
    print(Clock.default().now());
    print(Clock.from_str(" 42 ").now());
    u32 seven = 7;
    print(Clock.from(seven).now());
    print(Clock.from(true).now());
    // A failed parse is the Jux exception it surfaces as (ERRATA E134).
    try {
        print(Clock.from_str("x").now());
    } catch (NumberFormatException e) {
        print($"not a number: ${e.getMessage()}");
    }
}
"#;

const EXPECTED: &str = "100\n5\n0\n42\n7\n1\nnot a number: invalid digit found in string\n";

fn case_dir(tag: &str) -> PathBuf {
    let root = workspace_root();
    let dir = root.join("target").join(format!("it-trait-statics-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("stubs")).expect("creating the case directory");
    std::fs::create_dir_all(dir.join("c3fix").join("src")).expect("creating the crate");
    std::fs::write(dir.join("main.jux"), PROGRAM).expect("writing main.jux");
    let vendored = root.join("crates").join("juxc-driver").join("stubs").join("rust-std.jux.d");
    std::fs::copy(&vendored, dir.join("stubs").join("rust-std.jux.d")).expect("copying the vendored rust.std snapshot");
    std::fs::write(dir.join("stubs").join("c3fix.jux.d"), C3FIX_STUB).expect("writing c3fix.jux.d");
    std::fs::write(
        dir.join("c3fix").join("Cargo.toml"),
        "[package]\nname = \"c3fix\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n",
    )
    .expect("writing c3fix/Cargo.toml");
    std::fs::write(dir.join("c3fix").join("src").join("lib.rs"), C3FIX_LIB).expect("writing c3fix/src/lib.rs");
    dir
}

fn emit(dir: &Path) -> (PathBuf, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg(dir.join("main.jux"))
        .env("JUX_STUBS_DIR", dir.join("stubs"))
        .env("JUX_SELFCHECK", "1")
        .output()
        .expect("running juxc");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "juxc refused the program:\n{said}");
    let crate_dir = dir.join("target").join(".rust-build");
    let rust = std::fs::read_to_string(crate_dir.join("src").join("main.rs"))
        .unwrap_or_else(|e| panic!("reading the emitted main.rs: {e}\njuxc said:\n{said}"));
    (crate_dir, rust)
}

#[test]
fn trait_statics_are_called_through_their_trait_and_run() {
    let dir = case_dir("run");
    let (crate_dir, rust) = emit(&dir);
    let flat = rust.split_whitespace().collect::<Vec<_>>().join(" ");
    for want in [
        "<c3fix::Clock as c3fix::Tick>::origin()",
        "<c3fix::Clock as c3fix::Tick>::STEP",
        "<c3fix::Clock as std::default::Default>::default()",
        "<c3fix::Clock as std::str::FromStr>::from_str(",
        "<c3fix::Clock as std::convert::From<_>>::from(",
    ] {
        assert!(flat.contains(want), "expected `{want}` in the emitted Rust:\n{rust}");
    }
    let manifest = crate_dir.join("Cargo.toml");
    let toml = std::fs::read_to_string(&manifest).expect("reading the emitted Cargo.toml");
    let krate = dir.join("c3fix").to_string_lossy().replace('\\', "/");
    let toml = toml.replacen("[dependencies]\n", &format!("[dependencies]\nc3fix = {{ path = \"{krate}\" }}\n"), 1);
    std::fs::write(&manifest, toml).expect("adding the c3fix dependency");
    let out = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .env_remove("RUSTFLAGS")
        .output()
        .expect("running cargo");
    let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    assert!(
        out.status.success(),
        "the emitted crate did not build or run:\n{}\n{stdout}",
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(stdout, EXPECTED);
}

/// The trait never reaches the program's view: a static the stub does not
/// have is the ordinary `E0413`, with the statics the type does have, trait
/// ones included, and no trait's name.
#[test]
fn an_unknown_static_names_no_trait() {
    let dir = case_dir("unknown");
    std::fs::write(dir.join("main.jux"), "import rust.c3fix.Clock;\n\nvoid main() {\n    print(Clock.orgin().now());\n}\n")
        .expect("writing main.jux");
    let out = Command::new(env!("CARGO_BIN_EXE_juxc"))
        .arg("--check")
        .arg(dir.join("main.jux"))
        .env("JUX_STUBS_DIR", dir.join("stubs"))
        .env("JUX_SELFCHECK", "1")
        .output()
        .expect("running juxc");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(said.contains("[E0413] error: no static method `orgin` on class `rust.c3fix.Clock` -- did you mean `origin`?"), "{said}");
    assert!(!said.contains("Tick") && !said.contains("RustTrait"), "{said}");
}


// ---------------------------------------------------------------------------
// Blanket impls and `FromIterator` (sweep C4)
// ---------------------------------------------------------------------------

const C4FIX_LIB: &str = include_str!("../../../crates/juxc-bindgen/tests/fixtures/sweepc4-src/lib.rs");

/// What bindgen writes for `c4fix` (see `sweepc4_fixture.rs`).
const C4FIX_STUB: &str = r#"package rust.c4fix;

/** A counter: `Default + Clone`, `Named`, and built from numbers. */
@rust("c4fix::Clock")
@RustClone
@RustDebug
@RustPartialEq
@RustDefault
public class Clock implements Labelled, Maker, Named, Portable {
    @RustTrait("c4fix::Maker") public static final u32 KIND;
    @RustDefault public Clock();
    public u32 now();
    @RustTrait("std::default::Default") public static Clock default();
    @RustTrait("std::iter::FromIterator<_>") public static Clock from_iter(Vec<u32> iter);
    @RustTrait("c4fix::Maker") public static Clock make_default();
    @RustTrait("c4fix::Labelled") public static String label();
}

/** Blanket over one of the crate's own traits. */
@rust("c4fix::Labelled")
@RustBlanket("Named")
public interface Labelled {
    @RustStatic public String label();
}

/** Blanket over `Default + Clone`: decidable from a stub's derive facts. */
@rust("c4fix::Maker")
@RustBlanket("Default + Clone")
public interface Maker {
    @RustStatic public Self make_default();
}

/** A crate trait implemented directly. */
@rust("c4fix::Named")
public interface Named {
    public String name();
}

/** Neither `Default` nor `Named`: no blanket impl's static reaches it. */
@rust("c4fix::Plain")
@RustDebug
public struct Plain implements Portable {
    public u32 v;
}

/** Blanket over `Send`, which no stub records: covers nothing in the stub. */
@rust("c4fix::Portable")
@RustBlanket("Send")
public interface Portable {
    @RustStatic public u32 portable();
}
"#;

const C4_PROGRAM: &str = r#"import rust.c4fix.Clock;
import rust.std.Vec;

void main() {
    // A blanket impl over `Default + Clone`: a constant and a function.
    print(Clock.KIND);
    print(Clock.make_default().now());
    // A blanket impl over the crate's own trait `Named`.
    print(Clock.label());
    // `FromIterator<u32>`: any collection of `u32`.
    var ns = new Vec<u32>();
    u32 a = 2;
    u32 b = 3;
    ns.push(a);
    ns.push(b);
    print(Clock.from_iter(ns).now());
    print(ns.len());
}
"#;

const C4_EXPECTED: &str = "7\n0\nnamed\n5\n2\n";

#[test]
fn blanket_and_from_iter_statics_run() {
    let root = workspace_root();
    let dir = root.join("target").join("it-trait-statics-blanket");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("stubs")).unwrap();
    std::fs::create_dir_all(dir.join("c4fix").join("src")).unwrap();
    std::fs::write(dir.join("main.jux"), C4_PROGRAM).unwrap();
    std::fs::copy(
        root.join("crates").join("juxc-driver").join("stubs").join("rust-std.jux.d"),
        dir.join("stubs").join("rust-std.jux.d"),
    )
    .unwrap();
    std::fs::write(dir.join("stubs").join("c4fix.jux.d"), C4FIX_STUB).unwrap();
    std::fs::write(
        dir.join("c4fix").join("Cargo.toml"),
        "[package]\nname = \"c4fix\"\nversion = \"0.0.0\"\nedition = \"2021\"\npublish = false\n",
    )
    .unwrap();
    std::fs::write(dir.join("c4fix").join("src").join("lib.rs"), C4FIX_LIB).unwrap();
    let (crate_dir, rust) = emit(&dir);
    let flat = rust.split_whitespace().collect::<Vec<_>>().join(" ");
    for want in [
        "<c4fix::Clock as c4fix::Maker>::KIND",
        "<c4fix::Clock as c4fix::Maker>::make_default()",
        "<c4fix::Clock as c4fix::Labelled>::label()",
        "<c4fix::Clock as std::iter::FromIterator<_>>::from_iter(",
    ] {
        assert!(flat.contains(want), "expected `{want}` in the emitted Rust:\n{rust}");
    }
    let manifest = crate_dir.join("Cargo.toml");
    let toml = std::fs::read_to_string(&manifest).unwrap();
    let krate = dir.join("c4fix").to_string_lossy().replace('\\', "/");
    let toml = toml.replacen("[dependencies]\n", &format!("[dependencies]\nc4fix = {{ path = \"{krate}\" }}\n"), 1);
    std::fs::write(&manifest, toml).unwrap();
    let out = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .env_remove("RUSTFLAGS")
        .output()
        .expect("running cargo");
    let stdout = String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n");
    assert!(out.status.success(), "the emitted crate did not build or run:\n{}\n{stdout}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(stdout, C4_EXPECTED);
}
