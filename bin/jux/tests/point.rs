//! End-to-end test for Turn-1 class support.
//!
//! Runs `examples/point.jux`, which exercises:
//! - `class Point { … }` declaration with private int fields,
//! - one constructor with the `this.field = …` builder pattern,
//! - an immutable instance method (lowers to `&self`),
//! - a mutating instance method (lowers to `&mut self` via
//!   `body_writes_to_this` analysis),
//! - `new Point(…)` instantiation,
//! - field reads (`p.x`, `p.y`) and method calls (`p.shift(…)`)
//!   from outside the class.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn point_class_supports_fields_method_and_mutating_shift() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("point.jux");
    let emit_dir = workspace_root.join("target").join("it-point");

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
            "p = (3, 4)",
            "distance^2 = 25",
            "after shift: (13, 24)",
            "distance^2 = 745",
        ],
        "unexpected output:\n{stdout}",
    );
}
