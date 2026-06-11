//! End-to-end test for **numeric / char intrinsics and primitive-type
//! constants** (§K.11): integer abs/saturating/wrapping/checked forms
//! (width-faithful on `byte`), bit queries, radix formatting, float
//! sqrt/round-half-to-even/NaN handling/toFixed/totalOrder, the
//! `int.MAX_VALUE`-style constants, and char classifiers.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn numeric_intrinsics() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("numeric_intrinsics.jux");
    let emit_dir = workspace_root.join("target").join("it-numeric-intrinsics");

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
        [
            // Integer intrinsics.
            "7", "15", "10", "2", "ff", "101", "10", "100",
            // byte wrap/saturate + rotate.
            "-128", "127", "8",
            // Checked arithmetic.
            "true", "8", "true",
            // Float intrinsics.
            "1.5", "2", "3", "2", "true", "3.14", "true", "-1",
            // Constants.
            "true", "true", "127", "true", "true", "true",
            // Char classifiers.
            "true", "true", "X", "false", "true", "65",
        ],
        "unexpected output:\n{stdout}",
    );
}
