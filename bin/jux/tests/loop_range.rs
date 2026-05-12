//! End-to-end integration test for milestone-7 (for-each over a range).
//!
//! Runs `jux run examples/loop_range.jux` and asserts stdout is `55`
//! (sum of 1..=10).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn for_each_inclusive_range_sums_to_55() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("loop_range.jux");
    let emit_dir = workspace_root.join("target").join("it-loop-range");

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
    assert!(
        stdout.trim().ends_with("55"),
        "expected stdout to end with '55', got: {stdout:?}",
    );
}
