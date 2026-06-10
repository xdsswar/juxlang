//! End-to-end test for **method overloading** (§T.3, Phase-1
//! count-based subset).
//!
//! Runs `examples/method_overloading.jux`: instance overloads with
//! differing return types (1/2/3-arg `add`), implicit-this calls
//! into both group members, static overloads, and an aliased
//! (wrapper) receiver dispatching both `put` overloads.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn method_overloading_count_based() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("method_overloading.jux");
    let emit_dir = workspace_root.join("target").join("it-method-overloading");

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
        ["15", "21", "sum=16", "24", "v7", "7kg", "14"],
        "unexpected output:\n{stdout}",
    );
}
