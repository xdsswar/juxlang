//! End-to-end integration test for milestone-5.
//!
//! Runs `jux run examples/sum.jux` and asserts:
//! 1. Exit status is success.
//! 2. Stdout is the string `15` (the sum of 1..=5).
//!
//! Exercises: int-returning function called as an expression, compound
//! assignment `+=`, `break` inside `while (true)`, and an int argument
//! to print.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn sum_until_five_prints_fifteen() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("sum.jux");
    let emit_dir = workspace_root.join("target").join("it-sum");

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
    let trimmed = stdout.trim();
    assert!(
        trimmed.ends_with("15"),
        "expected stdout to end with '15', got: {trimmed:?}",
    );
}
