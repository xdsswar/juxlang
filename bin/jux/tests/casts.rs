//! End-to-end integration test for milestone-12 (`as` casts).
//!
//! Runs `jux run examples/casts.jux` and asserts stdout contains
//! `12` and `42` in order.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn casts_prints_sum_and_widened_value() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("casts.jux");
    let emit_dir = workspace_root.join("target").join("it-casts");

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
    assert_eq!(lines.as_slice(), ["12", "42"], "unexpected output:\n{stdout}");
}
