//! End-to-end test for **tuples** (§5.3).
//!
//! Runs `examples/tuples.jux`: tuple return types, tuple literals,
//! `.0` / `.1` element access, destructuring with `_` skip, and a
//! locally-built tuple from mixed expressions.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn tuples_literals_access_destructuring() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("tuples.jux");
    let emit_dir = workspace_root.join("target").join("it-tuples");

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
        ["3", "2", "3", "1", "jux v1", "5", "sum"],
        "unexpected output:\n{stdout}",
    );
}
