//! End-to-end test for **withTimeout** (JUX-ASYNC v2 §18.1.9):
//! in-time work returns its value; an overrunning task raises a
//! catchable TimeoutException.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn with_timeout_success_and_timeout() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("with_timeout.jux");
    let emit_dir = workspace_root.join("target").join("it-with-timeout");

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
        ["42", "timed out: operation timed out"],
        "unexpected output:\n{stdout}",
    );
}
