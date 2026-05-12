//! End-to-end integration test for milestone-13 (sizeof).
//!
//! Runs `jux run examples/sizing.jux` and asserts stdout contains the
//! expected sizes for each primitive plus the value-form count.
//! Type form: byte=1, short=2, i32=4, long=8, double=8, bool=1.
//! Value form: count is i32 → 4.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn sizing_prints_primitive_sizes_and_value_size() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("sizing.jux");
    let emit_dir = workspace_root.join("target").join("it-sizing");

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
        ["1", "2", "4", "8", "8", "1", "4"],
        "unexpected output:\n{stdout}",
    );
}
