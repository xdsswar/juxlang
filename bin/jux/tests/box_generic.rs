//! End-to-end test for Turn-1 generics.
//!
//! Runs `examples/box_generic.jux`. Exercises:
//! - Generic class declaration `class Box<T>`.
//! - Generic field (`T value;`).
//! - Generic constructor + method (`T get()`).
//! - Explicit instantiation `new Box<int>(42)` via Rust turbofish.
//! - Implicit instantiation `new Box(7)` via Rust inference.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn generic_box_class_handles_explicit_and_inferred_instantiation() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("box_generic.jux");
    let emit_dir = workspace_root.join("target").join("it-box-generic");

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
    assert_eq!(lines.as_slice(), ["42", "7"], "unexpected output:\n{stdout}");
}
