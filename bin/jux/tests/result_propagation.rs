//! End-to-end test for **Result/Option core types + the `?`
//! operator** (§K.3 / §K.4 / §X.4.1).
//!
//! Runs `examples/result_propagation.jux`: Result construction and
//! queries, `?` propagation through a Result-returning chain (both
//! the Ok and Err paths), and `?` on a nullable with null
//! short-circuit.

use std::path::PathBuf;
use std::process::Command;

#[test]
fn result_option_and_question_operator() {
    let jux = env!("CARGO_BIN_EXE_jux");
    let workspace_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf();

    let source = workspace_root.join("examples").join("result_propagation.jux");
    let emit_dir = workspace_root.join("target").join("it-result-propagation");

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
        ["port is 80", "true", "hey!", "true"],
        "unexpected output:\n{stdout}",
    );
}
