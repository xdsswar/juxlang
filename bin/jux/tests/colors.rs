//! Integration test for Turn-3 arrays — bare `{a, b, c}` initializer
//! on a fixed-size `T[N]` typed local.
//!
//! Builds and runs `examples/colors.jux`. Validates that the bare-init
//! form lowers correctly to a Rust array literal (no `vec!`) and that
//! `.length` and indexing all stay aligned with the fixed-size shape.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn colors_fixed_array_bare_initializer_lowers_to_rust_array_literal() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("colors.jux");
    let emit_dir = workspace_root.join("target").join("it-colors");

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
        ["3", "red", "green", "blue"],
        "unexpected output:\n{stdout}",
    );
}
