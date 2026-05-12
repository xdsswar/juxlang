//! End-to-end test for Turn-2 bounded type params.
//!
//! Runs `examples/greeting_box.jux`. Exercises:
//! - `<T extends Animal & Greeter>` — multi-bound with one class + one
//!   interface.
//! - Class bound → marker trait `AnimalKind`, transitively implemented
//!   by `Polite` (which extends Animal).
//! - Interface bound → direct Rust trait reference; `T.greet()` calls
//!   dispatch through the trait.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn bounded_box_accepts_subclass_that_implements_interface() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("greeting_box.jux");
    let emit_dir = workspace_root.join("target").join("it-greeting-box");

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
        ["item says: How do you do?"],
        "unexpected output:\n{stdout}",
    );
}
