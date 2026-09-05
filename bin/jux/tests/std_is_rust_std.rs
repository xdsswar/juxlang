//! End-to-end test that the Jux standard library IS Rust's. The prelude types
//! (`Vec`, `HashMap`, `String`) resolve with NO import, anything else comes in
//! by name, and both the single and the GROUPED import form lower to the real
//! Rust path — the grouped one used to emit `use rust::std::X;`, a module that
//! does not exist, even though the README documents that spelling.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn the_standard_library_is_the_rust_standard_library() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("std_is_rust_std.jux");
    let emit_dir = workspace_root.join("target").join("it-std-is-rust-std");

    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(&emit_dir)
        .arg(&source)
        .output()
        .expect("spawn jux");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success(),
        "jux exited with {:?}\nstderr:\n{stderr}\nstdout:\n{stdout}",
        output.status.code(),
    );
    let lines: Vec<&str> = stdout.lines().map(str::trim).filter(|s| !s.is_empty()).collect();
    assert_eq!(
        lines.as_slice(),
        ["3", "3", "1", "36", "5", "2", "2"],
        "unexpected output:\n{stdout}",
    );
}
