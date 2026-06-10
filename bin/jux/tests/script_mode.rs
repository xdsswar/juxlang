//! End-to-end test for **script mode** (§E.1.1 top-level statements)
//! and **if-expressions** (§A.2.9 value form).
//!
//! Runs `examples/script_mode.jux`: bare top-level statements wrapped
//! into a synthetic `main`, declarations mixed between statements,
//! a value-position `if (c) a else b`, and a chained
//! `else if` expression.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn script_mode_and_if_expressions() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("script_mode.jux");
    let emit_dir = workspace_root.join("target").join("it-script-mode");

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
        ["hello from script", "big", "42", "B", "0", "1", "2"],
        "unexpected output:\n{stdout}",
    );
}
