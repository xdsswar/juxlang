//! End-to-end test for three generic/nullable interactions that previously
//! miscompiled (gap scan N4/N6/N7):
//!   N4 — generic class implementing a generic interface (call-position
//!        turbofish in the inherent-forwarding shim);
//!   N6 — nullable generic field from a nullable ctor param (no double-`Some`);
//!   N7 — `?.` safe-call to a String stdlib method (routes through the stdlib
//!        method mapping).

use std::path::PathBuf;
use std::process::Command;

#[test]
fn generic_iface_nullable() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("generic_iface_nullable.jux");
    let emit_dir = workspace_root.join("target").join("it-generic-iface-nullable");

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
        ["42", "hi", "5", "-1", "5", "-1"],
        "unexpected output:\n{stdout}",
    );
}
