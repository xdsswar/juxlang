//! End-to-end test for the CRM showcase demo.
//!
//! Runs `examples/crm_demo.jux` — a capability tour: generics (`Box<T>`,
//! `Vec<Todo>`), an interface, the builder pattern (`return this`), enums +
//! `switch` with bare (Java-style) variant labels, for-each over a wrapper-class
//! collection, and int->double promotion for the completion percentage.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn crm_demo_compiles_and_runs() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("crm_demo.jux");
    let emit_dir = workspace_root.join("target").join("it-crm-demo");

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
            "=== Jux CRM ===",
            "!!! Design logo [done]",
            "! Write docs [active]",
            "!! Ship v1 [done]",
            "!!! Email leads [pending]",
            "--- 2/4 complete (50%) ---",
        ],
        "unexpected output:\n{stdout}",
    );
}
