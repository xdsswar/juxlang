//! End-to-end test that Rust's reserved words are ordinary Jux names. `type`,
//! `match`, `loop`, `box`, `mod`, `impl` and `trait` are unremarkable
//! identifiers in Java and C#, and the backend emits them as Rust raw
//! identifiers. They used to be rejected outright, which leaked Rust's
//! keyword list into Jux's surface; only the four Rust cannot escape at all
//! (`self`, `Self`, `crate`, `super`) are reserved.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn rust_keywords_are_ordinary_jux_names() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("rust_keyword_names.jux");
    let emit_dir = workspace_root.join("target").join("it-rust-keyword-names");

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
        ["3", "t", "3", "9", "9", "5", "7"],
        "unexpected output:\n{stdout}",
    );
}
