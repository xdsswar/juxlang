//! End-to-end test for **enum bodies** (§A.2.5) — methods and
//! constants after the variant list's `;` terminator (was: a parser
//! rejection "methods aren't supported yet").
//!
//! Runs `examples/enum_methods.jux`: payload dispatch via
//! `switch (this)`, method-calls-method through `this`, and an enum
//! constant lowered to an associated const.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn enum_methods_and_constants() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("enum_methods.jux");
    let emit_dir = workspace_root.join("target").join("it-enum-methods");

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
        ["12.56", "12", "true", "false", "4"],
        "unexpected output:\n{stdout}",
    );
}
