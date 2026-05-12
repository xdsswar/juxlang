//! End-to-end test for Turn-1 records.
//!
//! Runs `examples/vector3.jux`. Exercises:
//! - Header-only record declarations (`record Vector3(double x, …)`).
//! - The auto-canonical constructor `Vector3::new(…)`.
//! - Public component access (`v.x`, `v.y`, `v.z`) via the existing
//!   field-access path.
//! - Value equality via derived `PartialEq`.
//! - A String-component record with auto-`.clone()` on read.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn vector3_record_value_equality_and_string_component() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("vector3.jux");
    let emit_dir = workspace_root.join("target").join("it-vector3");

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
        ["v = (1, 2, 3)", "v == u", "v != w", "name=Ada, age=36"],
        "unexpected output:\n{stdout}",
    );
}
