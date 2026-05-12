//! Integration test for Turn-1 arrays (fixed-size).
//!
//! Builds and runs `examples/squares.jux`, asserts the output is the
//! squares of 0..=9 — one per line — driving:
//!
//! - `int[10]` typed-local array declaration,
//! - `new int[10]` zero-initialized array creation,
//! - `squares[i] = i * i;` indexed assignment,
//! - `squares[i]` indexed read in a `print(...)` call,
//! - `squares.length` Java-style length access.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn squares_array_fills_and_reads_via_index_and_length() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("squares.jux");
    let emit_dir = workspace_root.join("target").join("it-squares");

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
        ["0", "1", "4", "9", "16", "25", "36", "49", "64", "81"],
        "unexpected output:\n{stdout}",
    );
}
