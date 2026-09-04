//! Regression: `import rust.std.*;` compiles and runs.
//!
//! A wildcard over a foreign stub package used to fall through to the generic
//! import renderer and emit `use rust::std::*;` -- a crate that does not exist,
//! so rustc rejected it with E0433 and the raw rustc error reached the user.
//! Each named member must expand to its real Rust path instead.
use std::path::PathBuf;
use std::process::Command;
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().and_then(|p| p.parent()).expect("ws").to_path_buf()
}
#[test]
fn wildcard_foreign_import_runs() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let out = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(root().join("target").join("it-import-wildcard-rust-std"))
        .arg(root().join("examples").join("import_wildcard_rust_std.jux"))
        .output()
        .expect("spawn");
    let so = String::from_utf8_lossy(&out.stdout);
    let se = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "exit {:?}\nstderr:\n{se}\nstdout:\n{so}", out.status.code());
    let lines: Vec<String> =
        so.lines().map(str::trim).filter(|s| !s.is_empty()).map(String::from).collect();
    assert_eq!(lines, ["vec=2 map=2 deque=2"]);

    // The bogus module path must never come back: assert on the emitted Rust,
    // not just on the exit status, so a regression is named rather than merely
    // observed as a build failure.
    let emitted = root()
        .join("target")
        .join("it-import-wildcard-rust-std")
        .join("src")
        .join("main.rs");
    let rust = std::fs::read_to_string(&emitted).expect("emitted main.rs");
    assert!(!rust.contains("use rust::std"), "emitted the non-existent `rust::std` module:\n{rust}");
    assert!(rust.contains("use std::collections::HashMap;"), "missing real HashMap path");
    assert!(rust.contains("use std::vec::Vec;"), "missing real Vec path");
}
