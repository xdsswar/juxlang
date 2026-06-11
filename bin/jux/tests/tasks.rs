//! End-to-end test for **`spawn(f)` → Task<T>** (JUX-ASYNC v2
//! §18.1.3 / §18.1.4): expression and block lambdas, blockingGet,
//! cancel, and `await task` fan-in inside an async function.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn spawn_tasks() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("tasks.jux");
    let emit_dir = workspace_root.join("target").join("it-tasks");

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
        ["42", "10", "cancelled ok", "42"],
        "unexpected output:\n{stdout}",
    );
}
