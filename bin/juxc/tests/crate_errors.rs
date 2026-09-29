//! The crate-boundary sweep (ERRATA E1XX-SWEEPB) against a real crate.
//!
//! The fixture crate `sbfix` (`crates/juxc-bindgen/tests/fixtures/sweepb-src`)
//! is in naga's shape: `front::wgsl::Error`, `front::spv::Error`,
//! `back::spv::Error` and the trait `de::Error` are four items of one simple
//! name in one crate, a `Diagnostic` converts from two of them, and its error
//! types are each shaped like one Jux exception or another. Its stub is
//! generated here from the checked-in rustdoc JSON exactly as the driver
//! generates a dependency's; the programs are checked, and built and RUN
//! against the real crate.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/juxc")
        .to_path_buf()
}

fn fixtures() -> PathBuf {
    workspace_root().join("crates").join("juxc-bindgen").join("tests").join("fixtures")
}

/// The crate's stub and its nested packages, rendered as the driver writes
/// them: `(file name, text)`.
fn stub_files() -> Vec<(String, String)> {
    let json = std::fs::read_to_string(fixtures().join("sweepb.rustdoc.json")).expect("the fixture JSON");
    let jsons = [("sbfix", json.as_str())];
    let fam = juxc_bindgen::family::FamilyPaths::read("sbfix", &jsons).expect("the crate reads");
    let stub = juxc_bindgen::ingest::generate_family(&jsons, "rust.sbfix", &fam, &std::collections::HashMap::new())
        .expect("the crate ingests");
    let mut files = vec![("sbfix.jux.d".to_string(), juxc_bindgen::render_stub(&stub))];
    for nested in &stub.nested {
        files.push((format!("{}.jux.d", nested.package), juxc_bindgen::render_stub(nested)));
    }
    files
}

/// A case directory: `main.jux`, the stubs (vendored `rust.std` plus the
/// crate's), and the crate.
fn case_dir(tag: &str, program: &str) -> PathBuf {
    let root = workspace_root();
    let dir = root.join("target").join(format!("it-crate-errors-{tag}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("stubs")).expect("creating the case directory");
    std::fs::write(dir.join("main.jux"), program).expect("writing main.jux");
    let vendored = root.join("crates").join("juxc-driver").join("stubs").join("rust-std.jux.d");
    std::fs::copy(&vendored, dir.join("stubs").join("rust-std.jux.d")).expect("copying the vendored rust.std snapshot");
    for (name, text) in stub_files() {
        std::fs::write(dir.join("stubs").join(name), text).expect("writing a stub");
    }
    let src = dir.join("sbfix").join("src");
    std::fs::create_dir_all(&src).expect("creating the fixture crate");
    std::fs::write(
        dir.join("sbfix").join("Cargo.toml"),
        "[package]\nname = \"sbfix\"\nversion = \"0.1.0\"\nedition = \"2021\"\npublish = false\n\n[workspace]\n",
    )
    .expect("writing the fixture Cargo.toml");
    let lib = std::fs::read_to_string(fixtures().join("sweepb-src").join("lib.rs")).expect("reading the fixture crate");
    std::fs::write(src.join("lib.rs"), lib).expect("writing the fixture crate");
    dir
}

/// `juxc` on the case, with its stubs: `(succeeded, what it said)`.
fn juxc(dir: &Path, check_only: bool) -> (bool, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_juxc"));
    if check_only {
        cmd.arg("--check");
    }
    let out = cmd
        .arg(dir.join("main.jux"))
        .env("JUX_STUBS_DIR", dir.join("stubs"))
        .env("JUX_SELFCHECK", "1")
        .output()
        .expect("running juxc");
    let said = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
        .replace("\r\n", "\n");
    (out.status.success(), said)
}

/// Lower the program, link the emitted crate against the fixture crate and
/// run it: its exit status, standard output and standard error.
fn build_and_run(dir: &Path) -> (Option<i32>, String, String) {
    let (ok, said) = juxc(dir, false);
    assert!(ok, "juxc refused the program:\n{said}");
    let crate_dir = dir.join("target").join(".rust-build");
    let manifest = crate_dir.join("Cargo.toml");
    let toml = std::fs::read_to_string(&manifest).expect("reading the emitted Cargo.toml");
    let lib = dir.join("sbfix").to_string_lossy().replace('\\', "/");
    let toml = toml.replacen("[dependencies]\n", &format!("[dependencies]\nsbfix = {{ path = \"{lib}\" }}\n"), 1);
    std::fs::write(&manifest, toml).expect("adding the sbfix dependency");
    let built = Command::new(env!("CARGO"))
        .args(["build", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .env_remove("RUSTFLAGS")
        .output()
        .expect("running cargo");
    assert!(built.status.success(), "the emitted crate did not build:\n{}", String::from_utf8_lossy(&built.stderr));
    let out = Command::new(env!("CARGO"))
        .args(["run", "--quiet", "--manifest-path"])
        .arg(&manifest)
        .env_remove("RUSTFLAGS")
        .current_dir(dir)
        .output()
        .expect("running cargo");
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).replace("\r\n", "\n"),
        String::from_utf8_lossy(&out.stderr).replace("\r\n", "\n"),
    )
}

fn assert_no_rust(text: &str) {
    assert!(
        juxc_diagnostics::leak::find_rust_leak(text).is_none(),
        "shows Rust: {:?}\n{text}",
        juxc_diagnostics::leak::find_rust_leak(text)
    );
}

/// §G.4.1: each `Error` of the crate is named by its module path, every
/// signature takes the exact one, and `Diagnostic` converts from the one its
/// `From` impl names (the marker is qualified too).
#[test]
fn every_error_of_the_crate_is_nameable_and_each_signature_takes_its_own() {
    let dir = case_dir(
        "names",
        r#"
import rust.sbfix.Module;
import rust.sbfix.report;
import rust.sbfix.front.wgsl.Error;
import rust.sbfix.front.wgsl.parse;
import rust.sbfix.back.spv.Error as WriteError;
import rust.sbfix.write;

public void main() {
    try {
        Module m = parse("fn main() {\n    !\n}");
        print(m.name);
    } catch (Error e) {
        print(Module.describe_parse_error(e));
        print(e.line());
        print(report(e));
    }
    Module shader = parse("shader");
    print(shader.name);
    try {
        print(write(shader, "main", "f64"));
    } catch (WriteError e) {
        print(Module.is_capability_error(e));
    }
    Error made = new Error((u32) 3, "made by hand");
    print(report(made));
    // Written by their fully-qualified names: a variant, a function, a
    // `catch`.
    print(rust.sbfix.front.spv.Error.Truncated);
    Vec<u32> words = new Vec<u32>();
    words.push((u32) 7);
    try {
        print(rust.sbfix.front.spv.parse(words).name);
    } catch (rust.sbfix.front.spv.Error e) {
        print(e);
    }
}
"#,
    );
    let (code, stdout, stderr) = build_and_run(&dir);
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    assert_eq!(
        stdout,
        "unexpected token at line 2\n2\nwgsl: unexpected token (line 2)\nshader\ntrue\nwgsl: made by hand (line 3)\n\
         LibraryException: the module is truncated\nLibraryException: bad magic number 0x7\n",
        "{stderr}"
    );
}

/// Importing two of the crate's `Error`s by their simple name is `E0303`,
/// naming both.
#[test]
fn importing_two_errors_of_the_crate_is_e0303() {
    let dir = case_dir(
        "both",
        r#"
import rust.sbfix.front.wgsl.Error;
import rust.sbfix.back.spv.Error;

public void main() {
    print(1);
}
"#,
    );
    let (ok, said) = juxc(&dir, true);
    assert!(!ok, "importing both `Error`s must be refused:\n{said}");
    assert!(said.contains("[E0303]"), "{said}");
    assert!(said.contains("rust.sbfix.front.wgsl.Error") && said.contains("rust.sbfix.back.spv.Error"), "{said}");
}

/// Two wildcards that each bring an `Error` are fine until `Error` is used.
#[test]
fn two_wildcards_with_an_error_each_are_e0303_at_a_use() {
    let unused = case_dir(
        "wild-unused",
        r#"
import rust.sbfix.front.wgsl.*;
import rust.sbfix.back.spv.*;

public void main() {
    print(1);
}
"#,
    );
    let (ok, said) = juxc(&unused, true);
    assert!(ok, "an ambiguous name nobody uses is not an error:\n{said}");
    let used = case_dir(
        "wild-used",
        r#"
import rust.sbfix.front.wgsl.*;
import rust.sbfix.back.spv.*;

public void main() {
    Error e = new Error((u32) 1, "x");
    print(1);
}
"#,
    );
    let (ok, said) = juxc(&used, true);
    assert!(!ok, "a use of the ambiguous `Error` must be refused:\n{said}");
    assert!(said.contains("[E0303]"), "{said}");
    assert!(said.contains("rust.sbfix.front.wgsl.Error") && said.contains("rust.sbfix.back.spv.Error"), "{said}");
}

/// Bindgen G.5.4: every error of the crate is the closest Jux exception its
/// shape says, caught by that class, and shown as it wherever it is printed.
#[test]
fn each_crate_error_is_the_jux_exception_its_shape_says() {
    let dir = case_dir(
        "shapes",
        r#"
import rust.sbfix.Module;
import rust.sbfix.load_config;
import rust.sbfix.decode;
import rust.sbfix.parse_version;
import rust.sbfix.parse_decimal;
import rust.sbfix.wait_for;
import rust.sbfix.lookup;
import rust.sbfix.refuse;
import rust.sbfix.VersionError;
import rust.sbfix.ConfigError;
import rust.sbfix.write;
import rust.sbfix.back.spv.Error as WriteError;

Result<int, VersionError> version(String text) {
    try {
        return Result.Ok(parse_version(text).major as int);
    } catch (VersionError e) {
        return Result.Err(e);
    }
}

public void main() {
    Module m = new Module();
    try {
        print(load_config("no/such/settings.toml"));
    } catch (FileNotFoundException e) {
        print("settings: file not found");
    }
    try {
        print(write(m, "main", "locked"));
    } catch (AccessDeniedException e) {
        print($"write: access denied: ${e.getMessage()}");
    }
    try {
        print(write(m, "missing", "x"));
    } catch (NoSuchElementException e) {
        print($"write: no such element: ${e.getMessage()}");
    }
    try {
        print(write(m, "slow", "x"));
    } catch (TimeoutException e) {
        print($"write: timeout: ${e.getMessage()}");
    }
    try {
        print(write(m, "main", "f64"));
    } catch (UnsupportedOperationException e) {
        print($"write: unsupported: ${e.getMessage()}");
    }
    try {
        print(decode("oops").name);
    } catch (FormatException e) {
        print($"decode: format: ${e.getMessage()}");
    }
    try {
        print(parse_version("one").major);
    } catch (IllegalArgumentException e) {
        print($"version: illegal argument: ${e.getMessage()}");
    }
    try {
        print(parse_decimal("1.2.3").hundredths);
    } catch (NumberFormatException e) {
        print($"decimal: number format: ${e.getMessage()}");
    }
    try {
        print(wait_for((u32) 5, (u32) 1));
    } catch (TimeoutException e) {
        print($"wait: timeout: ${e.getMessage()}");
    }
    try {
        print(lookup("two"));
    } catch (NoSuchElementException e) {
        print($"lookup: no such element: ${e.getMessage()}");
    }
    try {
        print(lookup("poison"));
    } catch (LibraryException e) {
        print($"lookup: library ${e.getLibrary()}: ${e.getMessage()}");
    }
    try {
        refuse();
    } catch (LibraryException e) {
        print($"refuse: library ${e.getLibrary()}: ${e.getMessage()}");
    }
    // The library's own value, where the program shows it.
    try {
        print(write(m, "main", "locked"));
    } catch (WriteError e) {
        print(e);
        print($"interpolated: ${e}");
    }
    try {
        print(load_config("no/such/settings.toml"));
    } catch (ConfigError e) {
        print($"${e}".startsWith("FileNotFoundException: cannot load the settings: "));
    }
    print(version("2.1"));
    print(version("one"));
}
"#,
    );
    let (code, stdout, stderr) = build_and_run(&dir);
    assert_eq!(code, Some(0), "{stdout}{stderr}");
    assert_eq!(
        stdout,
        "settings: file not found\n\
         write: access denied: writing failed: output locked\n\
         write: no such element: no entry point named missing\n\
         write: timeout: the writer timed out\n\
         write: unsupported: capability f64 is not supported\n\
         decode: format: expected `name:` at column 1\n\
         version: illegal argument: `one` is not a version\n\
         decimal: number format: invalid decimal literal\n\
         wait: timeout: deadline has elapsed\n\
         lookup: no such element: no key two\n\
         lookup: library sbfix: the table is poisoned\n\
         refuse: library sbfix: the widget refused\n\
         AccessDeniedException: writing failed: output locked\n\
         interpolated: AccessDeniedException: writing failed: output locked\n\
         true\n\
         Ok(2)\n\
         Err(IllegalArgumentException: `one` is not a version)\n",
        "{stderr}"
    );
    assert_no_rust(&stdout);
}

/// An uncaught crate error ends the program under the Jux exception its shape
/// says, with the `.jux` line and status 101, never its Rust type name.
#[test]
fn an_uncaught_crate_error_is_reported_as_its_jux_exception() {
    let dir = case_dir(
        "uncaught",
        r#"
import rust.sbfix.lookup;

public void main() {
    print("before");
    print(lookup("zzz"));
}
"#,
    );
    let (code, stdout, stderr) = build_and_run(&dir);
    assert_eq!(code, Some(101), "{stdout}{stderr}");
    assert_eq!(stdout, "before\n");
    assert!(
        stderr.contains("Exception in thread \"main\" jux.std.exceptions.NoSuchElementException: no key zzz"),
        "{stderr}"
    );
    assert!(stderr.contains("main.jux:6"), "the report names the .jux line: {stderr}");
    assert!(!stderr.contains("LookupError"), "{stderr}");
    assert_no_rust(&stderr);
}
