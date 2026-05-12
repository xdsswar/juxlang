//! End-to-end integration test for milestone-4 (while + assignment).
//!
//! Runs `jux run examples/countdown.jux` and asserts:
//! 1. Exit status is success.
//! 2. Stdout contains the countdown sequence (`3`, `2`, `1`) and the
//!    terminator `liftoff`.
//!
//! This is the canary for the loop machinery — when this regresses,
//! something in the while-condition / assignment / mutation-analysis
//! path has broken.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn countdown_prints_three_two_one_liftoff() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("countdown.jux");
    let emit_dir = workspace_root.join("target").join("it-countdown");

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
    for needle in ["3", "2", "1", "liftoff"] {
        assert!(
            stdout.contains(needle),
            "stdout missing '{needle}':\n{stdout}",
        );
    }
    // Sanity: 3 should appear before liftoff in the output.
    let p3 = stdout.find('3').unwrap();
    let p_liftoff = stdout.find("liftoff").unwrap();
    assert!(
        p3 < p_liftoff,
        "expected '3' to print before 'liftoff', got:\n{stdout}",
    );
}
