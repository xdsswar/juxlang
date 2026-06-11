//! End-to-end test for **catch-body control flow vs `finally`**
//! (§X.3.2) — the deferred edge from the original try/finally work.
//!
//! Runs `examples/catch_finally_order.jux`:
//! - a `return` inside a catch runs the finally BEFORE returning
//!   (`handling`, `cleanup A`, then `7`);
//! - a `throw` inside a catch runs the finally BEFORE propagating
//!   (`cleanup B` before the outer handler's print).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn catch_return_and_throw_run_finally_first() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("catch_finally_order.jux");
    let emit_dir = workspace_root.join("target").join("it-catch-finally");

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
            "handling",
            "cleanup A",
            "7",
            "cleanup B",
            "outer caught: second",
            "9",
        ],
        "unexpected output:\n{stdout}",
    );
}
