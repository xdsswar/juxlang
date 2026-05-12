//! Integration test for Turn-2 arrays (dynamic + initializer literal).
//!
//! Builds and runs `examples/names.jux`. Validates that `String[]`
//! typed locals, `new String[]{…}` initializer literals, `.length`,
//! and index access all work end-to-end. Expected stdout: the length
//! (3) followed by the three names.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn names_array_literal_prints_length_and_each_element() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("names.jux");
    let emit_dir = workspace_root.join("target").join("it-names");

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
        ["3", "Alice", "Bob", "Carol"],
        "unexpected output:\n{stdout}",
    );
}
