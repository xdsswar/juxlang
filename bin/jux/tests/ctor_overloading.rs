//! End-to-end test for **constructor overloading + `this(...)`
//! delegation** (§7.3.1).
//!
//! Runs `examples/ctor_overloading.jux`: count-based overload
//! selection (0/2/3-arg ctors), chained `this(...)` delegation
//! (0-arg → 2-arg → 3-arg), `super(...)` into an overloaded parent,
//! and a delegating constructor on an aliased (wrapper) class with
//! statements after the delegation.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn ctor_overloading_and_this_delegation() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("ctor_overloading.jux");
    let emit_dir = workspace_root.join("target").join("it-ctor-overloading");

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
            "origin (0, 0)",
            "custom (3, 4)",
            "labeled (5, 6)",
            "sub (1, 2)",
            "true",
            "built anon",
            "12",
        ],
        "unexpected output:\n{stdout}",
    );
}
