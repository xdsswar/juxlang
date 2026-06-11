//! End-to-end test for **try-expressions** (§X.3.1 / §X.3.3).
//!
//! Runs `examples/try_expression.jux`: success path yields the try
//! block's trailing expression, failure path yields the catch's,
//! and a multi-statement form with a typed catch clause.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn try_expression_values() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("try_expression.jux");
    let emit_dir = workspace_root.join("target").join("it-try-expression");

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
        ["42", "-1", "rejected: not a number"],
        "unexpected output:\n{stdout}",
    );
}
