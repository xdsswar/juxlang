//! End-to-end test for **switch `when` guards** (§A.2.8) and
//! **or-patterns** (§A.3).
//!
//! Runs `examples/switch_guards.jux`: guards over payload bindings
//! (`case Circle(var r) when r > 10.0`), guard-then-unguarded
//! fallthrough ordering, enum or-pattern union coverage (no wildcard),
//! literal or-patterns, and a guard over a top-level `var` bind.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn switch_guards_and_or_patterns() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("switch_guards.jux");
    let emit_dir = workspace_root.join("target").join("it-switch-guards");

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
            "big circle",
            "circle 2",
            "degenerate square",
            "square 3",
            "point",
            "base",
            "premium",
            "10",
            "-1",
            "99",
        ],
        "unexpected output:\n{stdout}",
    );
}
