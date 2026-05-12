//! End-to-end integration test for milestone-11 (bitwise operators).
//!
//! Runs `jux run examples/bitops.jux` and asserts stdout contains the
//! five expected values: 255, 0, 255, 16, 16.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn bitops_prints_expected_values() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("bitops.jux");
    let emit_dir = workspace_root.join("target").join("it-bitops");

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
        ["255", "0", "255", "16", "16"],
        "unexpected output:\n{stdout}",
    );
}
