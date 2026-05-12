//! End-to-end test for Turn-1 interfaces.
//!
//! Runs `examples/shapes.jux`. Exercises:
//! - `interface Shape { … }` lowered to Rust `trait`.
//! - `class Circle implements Shape { … }` lowered to inherent impl
//!   plus a delegating `impl Shape for Circle`.
//! - Direct method dispatch via the inherent path (`c.area()`).
//! - String-return method via interpolation, threading the
//!   position-aware `String` mapping through interface boundaries.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn shape_interface_with_circle_and_square_implementations() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("shapes.jux");
    let emit_dir = workspace_root.join("target").join("it-shapes");

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
        ["Circle area = 12.56636", "Square area = 9"],
        "unexpected output:\n{stdout}",
    );
}
