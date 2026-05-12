//! End-to-end integration test for milestone-10 (logical operators).
//!
//! Runs `jux run examples/range_check.jux` and asserts the three
//! `inRange` calls print `true`, `false`, `false` in order.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn range_check_prints_true_false_false() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("range_check.jux");
    let emit_dir = workspace_root.join("target").join("it-range-check");

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

    let lines: Vec<&str> = stdout.lines().collect();
    let truthies: Vec<usize> = lines.iter().enumerate()
        .filter(|(_, l)| l.trim() == "true")
        .map(|(i, _)| i)
        .collect();
    let falsies: Vec<usize> = lines.iter().enumerate()
        .filter(|(_, l)| l.trim() == "false")
        .map(|(i, _)| i)
        .collect();
    assert_eq!(truthies.len(), 1, "expected exactly one `true` line: {stdout}");
    assert_eq!(falsies.len(), 2, "expected exactly two `false` lines: {stdout}");
    assert!(truthies[0] < falsies[0], "true should come before falses: {stdout}");
}
