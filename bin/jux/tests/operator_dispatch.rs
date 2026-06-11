//! End-to-end test for **user-operator dispatch** (§O.2.4):
//! `operator[]` reads, `operator[]=` writes (plus compound
//! read-modify-write), callable values via `operator()`, and unary
//! negation via zero-param `operator-`.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn index_call_and_unary_operator_dispatch() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("operator_dispatch.jux");
    let emit_dir = workspace_root.join("target").join("it-operator-dispatch");

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
        ["9", "14", "15", "-250"],
        "unexpected output:\n{stdout}",
    );
}
