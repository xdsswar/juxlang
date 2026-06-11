//! End-to-end test for **multi-catch** (`catch (E1 | E2 e)`, §X.3.6)
//! and **catch-by-supertype** (§X.3.4).
//!
//! Runs `examples/multi_catch.jux`: a two-type clause absorbing
//! either exception with one body, and a base `catch (Exception e)`
//! matching a subclass (IllegalStateException) — `Any::downcast` is
//! exact-type, so the emitter expands subtype arms explicitly.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn multi_catch_and_subtype_matching() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("multi_catch.jux");
    let emit_dir = workspace_root.join("target").join("it-multi-catch");

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
        ["transient: net down", "transient: too slow", "hard: other"],
        "unexpected output:\n{stdout}",
    );
}
